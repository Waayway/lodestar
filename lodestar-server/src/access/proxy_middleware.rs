//! Middleware of the router entrance: the trusted-peer gate, the viewer from the session
//! cookie and the router's mark, and the WebSocket Origin check.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::{Actor, Viewer};
use crate::auth::cookie::{SESSION_COOKIE, read_cookie};
use crate::auth::{AuthState, SessionId};
use crate::state::AppState;
use crate::{clock, db};

/// The header the router sets on requests that reached it over Tailscale.
pub const TAILNET_MARK_HEADER: &str = "x-homelab-via";
/// The only value of [`TAILNET_MARK_HEADER`] that counts, and only as its single value.
pub const TAILNET_MARK_VALUE: &str = "tailnet";

/// Middleware: plain 403 `forbidden` unless the peer is one of the trusted proxies. Needs
/// `into_make_service_with_connect_info::<SocketAddr>()` on the server.
pub async fn require_trusted_peer(
    State(auth): State<Arc<AuthState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Response {
    let peer_ip = peer.ip().to_canonical();
    if auth
        .config
        .trusted_proxies
        .iter()
        .any(|trusted| trusted.to_canonical() == peer_ip)
    {
        next.run(request).await
    } else {
        (StatusCode::FORBIDDEN, "forbidden").into_response()
    }
}

/// Middleware: inserts the request's `Viewer` (from the session cookie and the router's
/// mark; anonymous without a live session) and, when signed in, its `Actor`.
pub async fn load_viewer(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    // Exactly one value, and that one `tailnet`: none or several count as unmarked, so the
    // server doesn't depend on whether the router appends to or replaces a client's header.
    let mut marks = request.headers().get_all(TAILNET_MARK_HEADER).iter();
    let via_tailnet = match (marks.next(), marks.next()) {
        (Some(value), None) => value.as_bytes() == TAILNET_MARK_VALUE.as_bytes(),
        _ => false,
    };
    let session_id = read_cookie(request.headers(), SESSION_COOKIE)
        .and_then(|value| SessionId::from_cookie(&value));
    let session = match session_id {
        None => None,
        Some(id) => match db::find_session(&state.db, &id.hash(), clock::now()).await {
            Ok(session) => session,
            Err(error) => {
                eprintln!("lodestar-server: loading a session: {error:#}");
                None
            }
        },
    };
    let viewer = match &session {
        Some(session) => Viewer::from_session(session, via_tailnet),
        None => Viewer::anonymous(),
    };
    if let Some(name) = &viewer.name {
        request.extensions_mut().insert(Actor(name.clone()));
    }
    request.extensions_mut().insert(viewer);
    next.run(request).await
}

/// Middleware for the WebSocket upgrade: 403 unless `Origin` is exactly the public URL's
/// origin. Browsers always send it on a WebSocket; another site's page sends its own.
pub async fn require_public_origin(
    State(auth): State<Arc<AuthState>>,
    request: Request,
    next: Next,
) -> Response {
    let expected = auth.config.public_origin();
    let matches = request
        .headers()
        .get(header::ORIGIN)
        .is_some_and(|origin| !expected.is_empty() && origin.as_bytes() == expected.as_bytes());
    if matches {
        next.run(request).await
    } else {
        (StatusCode::FORBIDDEN, "forbidden").into_response()
    }
}
