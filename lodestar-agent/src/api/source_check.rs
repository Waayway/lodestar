use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::AppState;

/// Middleware: only the manager host may call the agent.
pub(super) async fn check_source(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Response {
    if state.guard.allows(peer.ip()).await {
        next.run(request).await
    } else {
        StatusCode::FORBIDDEN.into_response()
    }
}
