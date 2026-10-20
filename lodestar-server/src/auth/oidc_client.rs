use std::time::{Duration, Instant};

use anyhow::Context;

use openidconnect::core::{
    CoreAuthDisplay, CoreAuthPrompt, CoreAuthenticationFlow, CoreErrorResponseType,
    CoreGenderClaim, CoreJsonWebKey, CoreJweContentEncryptionAlgorithm, CoreJwsSigningAlgorithm,
    CoreProviderMetadata, CoreRevocableToken, CoreRevocationErrorResponse,
    CoreTokenIntrospectionResponse, CoreTokenType,
};
use openidconnect::{
    AuthorizationCode, Client, ClientId, ClientSecret, CsrfToken, DiscoveryError,
    EmptyExtraTokenFields, EndpointMaybeSet, EndpointNotSet, EndpointSet, IdTokenFields, IssuerUrl,
    Nonce, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RequestTokenError, Scope,
    StandardErrorResponse, StandardTokenResponse, TokenResponse,
};

use super::{GroupsClaim, LoginError, LoginIdentity, LoginStart, PendingLogin};
use crate::config::OidcConfig;

/// How long a fetched discovery document (and its JWKS) is reused.
pub const DISCOVERY_TTL: Duration = Duration::from_secs(3600);
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
/// Requested besides `openid`, which the client always adds.
const EXTRA_SCOPES: [&str; 2] = ["profile", "groups"];
const ALLOWED_ID_TOKEN_ALGS: [CoreJwsSigningAlgorithm; 2] = [
    CoreJwsSigningAlgorithm::EcdsaP256Sha256,
    CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256,
];

type GroupsTokenResponse = StandardTokenResponse<
    IdTokenFields<
        GroupsClaim,
        EmptyExtraTokenFields,
        CoreGenderClaim,
        CoreJweContentEncryptionAlgorithm,
        CoreJwsSigningAlgorithm,
    >,
    CoreTokenType,
>;

/// `CoreClient` with the `groups` claim, as built from discovery metadata.
type GroupsClient = Client<
    GroupsClaim,
    CoreAuthDisplay,
    CoreGenderClaim,
    CoreJweContentEncryptionAlgorithm,
    CoreJsonWebKey,
    CoreAuthPrompt,
    StandardErrorResponse<CoreErrorResponseType>,
    GroupsTokenResponse,
    CoreTokenIntrospectionResponse,
    CoreRevocableToken,
    CoreRevocationErrorResponse,
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

/// The OpenID Connect client of the router entrance: authorization code flow with PKCE
/// (S256), `state` and `nonce`; ID tokens signed ES256 or RS256.
pub struct OidcClient {
    config: OidcConfig,
    issuer: IssuerUrl,
    redirect_uri: RedirectUrl,
    http: reqwest::Client,
    /// The last successful discovery (metadata with JWKS) and when it was fetched.
    discovery: tokio::sync::Mutex<Option<(Instant, CoreProviderMetadata)>>,
}

impl OidcClient {
    /// `public_url` is the entrance's public URL; the redirect URI is its origin plus
    /// `/auth/callback`.
    pub fn new(config: OidcConfig, public_url: &str) -> anyhow::Result<Self> {
        let issuer = IssuerUrl::new(config.issuer.clone())
            .with_context(|| format!("oidc.issuer {:?} is not a URL", config.issuer))?;
        let public = reqwest::Url::parse(public_url.trim())
            .with_context(|| format!("public URL {public_url:?} is not a URL"))?;
        let redirect_uri = RedirectUrl::new(format!(
            "{}/auth/callback",
            public.origin().ascii_serialization()
        ))
        .context("building the redirect URI")?;
        // Following redirects would let the issuer point the client anywhere (SSRF).
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(HTTP_TIMEOUT)
            .build()
            .context("building the OpenID Connect HTTP client")?;
        Ok(OidcClient {
            config,
            issuer,
            redirect_uri,
            http,
            discovery: tokio::sync::Mutex::new(None),
        })
    }

    /// A fresh authorization request: the URL to send the browser to, its `state`, and the
    /// secrets the callback needs.
    pub async fn begin(&self, next: String, now: i64) -> Result<LoginStart, LoginError> {
        let client = self.client(None).await?;
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        let mut request = client.authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        );
        for scope in EXTRA_SCOPES {
            request = request.add_scope(Scope::new(scope.to_string()));
        }
        let (authorize_url, state, nonce) = request.set_pkce_challenge(pkce_challenge).url();
        Ok(LoginStart {
            authorize_url: authorize_url.to_string(),
            state: state.secret().clone(),
            pending: PendingLogin {
                nonce: nonce.secret().clone(),
                pkce_verifier: pkce_verifier.secret().clone(),
                next,
                created_at: now,
            },
        })
    }

    /// Exchanges the callback's `code` (with the client secret, read now, and the PKCE
    /// verifier) and verifies the ID token: signature, issuer, audience, expiry, nonce.
    pub async fn finish(
        &self,
        code: &str,
        pending: PendingLogin,
    ) -> Result<LoginIdentity, LoginError> {
        let secret = self.read_client_secret()?;
        let client = self.client(Some(secret)).await?;
        let response = client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .map_err(|error| LoginError::Misconfigured(format!("token endpoint: {error}")))?
            .set_pkce_verifier(PkceCodeVerifier::new(pending.pkce_verifier))
            .request_async(&self.http)
            .await
            .map_err(token_error)?;
        let id_token = response
            .id_token()
            .ok_or_else(|| LoginError::Refused("token response has no ID token".into()))?;
        let verifier = client
            .id_token_verifier()
            .set_allowed_algs(ALLOWED_ID_TOKEN_ALGS);
        let claims = id_token
            .claims(&verifier, &Nonce::new(pending.nonce))
            .map_err(|error| LoginError::Refused(format!("ID token: {error}")))?;
        let user = claims
            .preferred_username()
            .map(|name| name.as_str().to_string())
            .filter(|name| !name.is_empty())
            .ok_or_else(|| LoginError::Refused("ID token has no preferred_username".into()))?;
        Ok(LoginIdentity {
            user,
            groups: claims.additional_claims().groups.clone(),
        })
    }

    fn read_client_secret(&self) -> Result<ClientSecret, LoginError> {
        let path = &self.config.client_secret_file;
        let text = std::fs::read_to_string(path).map_err(|error| {
            LoginError::Misconfigured(format!("reading client secret {}: {error}", path.display()))
        })?;
        let secret = text.trim();
        if secret.is_empty() {
            return Err(LoginError::Misconfigured(format!(
                "client secret {} is empty",
                path.display()
            )));
        }
        Ok(ClientSecret::new(secret.to_string()))
    }

    async fn client(&self, secret: Option<ClientSecret>) -> Result<GroupsClient, LoginError> {
        let metadata = self.metadata().await?;
        Ok(GroupsClient::from_provider_metadata(
            metadata,
            ClientId::new(self.config.client_id.clone()),
            secret,
        )
        .set_redirect_uri(self.redirect_uri.clone()))
    }

    /// Discovery metadata with its JWKS, fetched at most once per `DISCOVERY_TTL`.
    async fn metadata(&self) -> Result<CoreProviderMetadata, LoginError> {
        let mut cached = self.discovery.lock().await;
        if let Some((fetched_at, metadata)) = cached.as_ref()
            && fetched_at.elapsed() < DISCOVERY_TTL
        {
            return Ok(metadata.clone());
        }
        let metadata = CoreProviderMetadata::discover_async(self.issuer.clone(), &self.http)
            .await
            .map_err(discovery_error)?;
        *cached = Some((Instant::now(), metadata.clone()));
        Ok(metadata)
    }
}

fn discovery_error<E: std::error::Error>(error: DiscoveryError<E>) -> LoginError {
    match error {
        DiscoveryError::Request(source) => {
            LoginError::IssuerUnreachable(format!("discovery: {}", error_chain(&source)))
        }
        DiscoveryError::Response(status, _, _) => {
            LoginError::IssuerUnreachable(format!("discovery answered {status}"))
        }
        other => LoginError::Misconfigured(format!("discovery: {other}")),
    }
}

/// Only the errors' own texts: the server's error code, never the response body.
fn token_error<E: std::error::Error>(
    error: RequestTokenError<E, StandardErrorResponse<CoreErrorResponseType>>,
) -> LoginError {
    match error {
        RequestTokenError::Request(source) => {
            LoginError::IssuerUnreachable(format!("token endpoint: {}", error_chain(&source)))
        }
        RequestTokenError::ServerResponse(response) => {
            LoginError::Refused(format!("token endpoint: {}", response.error()))
        }
        RequestTokenError::Parse(_, _) => {
            LoginError::Refused("token endpoint: unreadable response".into())
        }
        RequestTokenError::Other(reason) => {
            LoginError::Refused(format!("token endpoint: {reason}"))
        }
    }
}

/// A transport error with its causes (`client error: …: Connection refused`).
fn error_chain(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}
