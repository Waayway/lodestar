use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};

use super::{Access, Actor, decide};
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
            request.extensions_mut().insert(Actor(actor));
            next.run(request).await
        }
        Access::Denied => (StatusCode::FORBIDDEN, "forbidden").into_response(),
    }
}
