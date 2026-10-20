use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    Json,
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};

use super::{Access, Actor, Need, Viewer, decide};
use crate::config::AccessConfig;
use crate::tailscale::OnlineProvider;

#[derive(Clone)]
pub struct AccessState {
    pub provider: Arc<dyn OnlineProvider>,
    pub config: Arc<AccessConfig>,
}

/// Middleware: whois the peer, 403 unless allowed. Needs
/// `into_make_service_with_connect_info::<SocketAddr>()` on the server.
pub async fn require_access(
    State(state): State<AccessState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    mut request: Request,
    next: Next,
) -> Response {
    let identity = match *state.config {
        AccessConfig::Disabled => None,
        AccessConfig::Tailscale { .. } => state.provider.whois(peer.ip().to_canonical()).await,
    };
    match decide(&state.config, identity.as_ref()) {
        Access::Allowed { actor } => {
            request
                .extensions_mut()
                .insert(Viewer::tailnet(actor.clone()));
            request.extensions_mut().insert(Actor(actor));
            next.run(request).await
        }
        Access::Denied => (StatusCode::FORBIDDEN, "forbidden").into_response(),
    }
}

/// Middleware for a route group: refuse with `{"error": ...}` unless the request's `Viewer`
/// (put there by the entrance) meets `need`. A request without a viewer is refused as
/// anonymous.
pub async fn require_need(need: Need, request: Request, next: Next) -> Response {
    let permitted = match request.extensions().get::<Viewer>() {
        Some(viewer) => viewer.permits(need),
        None => Viewer::anonymous().permits(need),
    };
    match permitted {
        Ok(()) => next.run(request).await,
        Err((status, message)) => {
            (status, Json(serde_json::json!({ "error": message }))).into_response()
        }
    }
}
