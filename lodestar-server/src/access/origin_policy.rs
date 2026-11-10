//! Which `Origin` a browser request may carry on each listener.

use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::authority::{Authority, Scheme, single_header};
use super::host_allowlist::request_authority;
use crate::config::ProxyConfig;

/// What a sent `Origin` must be. Browsers send one on every WebSocket and on cross-origin
/// and (in current browsers) same-origin POSTs; an absent one is a non-browser client.
#[derive(Clone, Debug)]
pub enum OriginPolicy {
    /// The tailnet listener (plain http): `http://` and the request's own `Host`, so a page
    /// served under another of this server's names, or another port, is still another site.
    SameAsHost,
    /// The router entrance: the public URL's origin (`https`); `None` (an invalid public URL,
    /// refused at startup) matches nothing.
    Public(Option<Authority>),
}

impl OriginPolicy {
    pub fn public(proxy: &ProxyConfig) -> OriginPolicy {
        let authority = proxy.parse_public_url().ok().and_then(|url| {
            let host = url.host_str()?;
            let text = match url.port() {
                Some(port) => format!("{host}:{port}"),
                None => host.to_string(),
            };
            Authority::parse(&text)
        });
        OriginPolicy::Public(authority)
    }

    /// True when `Origin` is sent once and matches, or is absent and not `required`. A
    /// malformed value, `null`, or several `Origin` headers never pass.
    pub fn allows(&self, headers: &HeaderMap, uri: &Uri, required: bool) -> bool {
        let Some(origin) = single_header(headers, &header::ORIGIN) else {
            return false;
        };
        let Some(text) = origin else {
            return !required;
        };
        let Some((scheme, authority)) = Authority::parse_origin(text) else {
            return false;
        };
        match self {
            OriginPolicy::SameAsHost => {
                scheme == Scheme::Http
                    && request_authority(headers, uri)
                        .is_some_and(|own| authority.same_as(&own, Scheme::Http.default_port()))
            }
            OriginPolicy::Public(Some(expected)) => {
                scheme == Scheme::Https && authority.same_as(expected, Scheme::Https.default_port())
            }
            OriginPolicy::Public(None) => false,
        }
    }
}

/// Middleware for the tailnet WebSocket upgrade: plain 403 `forbidden` when a browser's
/// `Origin` is not the request's own (no `Origin`: a non-browser client, let through).
pub async fn require_origin(
    State(policy): State<OriginPolicy>,
    request: Request,
    next: Next,
) -> Response {
    if policy.allows(request.headers(), request.uri(), false) {
        next.run(request).await
    } else {
        (StatusCode::FORBIDDEN, "forbidden").into_response()
    }
}
