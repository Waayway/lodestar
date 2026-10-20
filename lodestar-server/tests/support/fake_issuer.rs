//! An OpenID Connect issuer on 127.0.0.1:0 for tests: discovery, a JWKS with one fixed
//! P-256 key, an authorize endpoint that only records the PKCE challenge, and a token
//! endpoint that hands out ID tokens prepared with `issue` (ES256, signed by hand).
//!
//! Flow in a test: `begin`, GET the authorize URL (records its `code_challenge`), `issue`
//! a code (bound to that challenge), `finish` with the code.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const KEY_ID: &str = "fake-key";
const CLIENT_ID: &str = "lodestar";
const SIGNING_KEY_BYTES: [u8; 32] = [7; 32];
const OTHER_KEY_BYTES: [u8; 32] = [9; 32];

pub struct FakeIssuer {
    shared: Arc<Shared>,
}

struct Shared {
    issuer: String,
    /// code → (signed ID token, PKCE challenge recorded at the time of `issue`)
    codes: Mutex<HashMap<String, (String, Option<String>)>>,
    last_challenge: Mutex<Option<String>>,
    last_client_secret: Mutex<Option<String>>,
    discovery_hits: AtomicUsize,
}

impl FakeIssuer {
    pub async fn start() -> FakeIssuer {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake issuer");
        let address: SocketAddr = listener.local_addr().expect("fake issuer address");
        let shared = Arc::new(Shared {
            issuer: format!("http://{address}"),
            codes: Mutex::new(HashMap::new()),
            last_challenge: Mutex::new(None),
            last_client_secret: Mutex::new(None),
            discovery_hits: AtomicUsize::new(0),
        });
        let app = Router::new()
            .route("/.well-known/openid-configuration", get(discovery))
            .route("/jwks", get(jwks))
            .route("/authorize", get(authorize))
            .route("/token", post(token))
            .with_state(shared.clone());
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("fake issuer serve");
        });
        FakeIssuer { shared }
    }

    pub fn issuer(&self) -> String {
        self.shared.issuer.clone()
    }

    /// Prepares `code` to be exchanged for an ID token with `claims` (`iss`, `aud`, `iat`
    /// and `exp` filled in unless given), bound to the last authorize request's challenge.
    pub fn issue(&self, code: &str, claims: Value) {
        self.issue_with_key(code, claims, &SIGNING_KEY_BYTES);
    }

    /// Like `issue`, but signed by a key that is not in the JWKS (same `kid`).
    pub fn issue_signed_by_other_key(&self, code: &str, claims: Value) {
        self.issue_with_key(code, claims, &OTHER_KEY_BYTES);
    }

    pub fn discovery_hits(&self) -> usize {
        self.shared.discovery_hits.load(Ordering::SeqCst)
    }

    pub fn last_client_secret(&self) -> Option<String> {
        self.shared.last_client_secret.lock().unwrap().clone()
    }

    fn issue_with_key(&self, code: &str, mut claims: Value, key_bytes: &[u8; 32]) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let object = claims
            .as_object_mut()
            .expect("claims must be a JSON object");
        object
            .entry("iss")
            .or_insert_with(|| json!(self.shared.issuer));
        object.entry("aud").or_insert_with(|| json!(CLIENT_ID));
        object.entry("iat").or_insert_with(|| json!(now));
        object.entry("exp").or_insert_with(|| json!(now + 300));
        let id_token = sign_es256(&claims, key_bytes);
        let challenge = self.shared.last_challenge.lock().unwrap().clone();
        self.shared
            .codes
            .lock()
            .unwrap()
            .insert(code.to_string(), (id_token, challenge));
    }
}

fn signing_key(bytes: &[u8; 32]) -> SigningKey {
    SigningKey::from_bytes(bytes.into()).expect("valid P-256 scalar")
}

/// Compact JWS: base64url(header).base64url(payload).base64url(r‖s).
fn sign_es256(claims: &Value, key_bytes: &[u8; 32]) -> String {
    let header = json!({"alg": "ES256", "typ": "JWT", "kid": KEY_ID});
    let signing_input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header.to_string()),
        URL_SAFE_NO_PAD.encode(claims.to_string())
    );
    let signature: Signature = signing_key(key_bytes).sign(signing_input.as_bytes());
    format!(
        "{signing_input}.{}",
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    )
}

async fn discovery(State(shared): State<Arc<Shared>>) -> Json<Value> {
    shared.discovery_hits.fetch_add(1, Ordering::SeqCst);
    let issuer = &shared.issuer;
    Json(json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{issuer}/authorize"),
        "token_endpoint": format!("{issuer}/token"),
        "jwks_uri": format!("{issuer}/jwks"),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["ES256"],
    }))
}

async fn jwks() -> Json<Value> {
    let public_key = p256::PublicKey::from(signing_key(&SIGNING_KEY_BYTES).verifying_key());
    let mut key = serde_json::to_value(public_key.to_jwk()).expect("JWK serializes");
    let object = key.as_object_mut().expect("JWK is an object");
    object.insert("kid".into(), json!(KEY_ID));
    object.insert("use".into(), json!("sig"));
    object.insert("alg".into(), json!("ES256"));
    Json(json!({ "keys": [key] }))
}

async fn authorize(
    State(shared): State<Arc<Shared>>,
    Query(query): Query<HashMap<String, String>>,
) -> &'static str {
    *shared.last_challenge.lock().unwrap() = query.get("code_challenge").cloned();
    "authorize"
}

async fn token(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let secret = basic_auth_password(&headers).or_else(|| form.get("client_secret").cloned());
    *shared.last_client_secret.lock().unwrap() = secret;

    let issued = form
        .get("code")
        .and_then(|code| shared.codes.lock().unwrap().remove(code));
    let Some((id_token, challenge)) = issued else {
        return invalid_grant();
    };
    let verifier_challenge = form
        .get("code_verifier")
        .map(|verifier| URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())));
    if challenge.is_none() || verifier_challenge != challenge {
        return invalid_grant();
    }
    Json(json!({
        "access_token": "fake-access-token",
        "token_type": "Bearer",
        "expires_in": 300,
        "id_token": id_token,
    }))
    .into_response()
}

fn invalid_grant() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": "invalid_grant" })),
    )
        .into_response()
}

/// The password of `Authorization: Basic …`, form-url-decoded (as oauth2 encodes it).
fn basic_auth_password(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let decoded = STANDARD.decode(value.strip_prefix("Basic ")?).ok()?;
    let credentials = String::from_utf8(decoded).ok()?;
    let (_, password) = credentials.split_once(':')?;
    let query = reqwest::Url::parse(&format!("http://decode/?p={password}")).ok()?;
    query
        .query_pairs()
        .find(|(key, _)| key == "p")
        .map(|(_, value)| value.into_owned())
}
