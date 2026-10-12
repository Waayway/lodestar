//! The dashboard HTTP surface: JSON API under `/api`, the WebSocket at `/api/ws`, and the
//! embedded frontend for everything else. Every route, the WebSocket upgrade and the static
//! files included, sits behind `require_access`; state-changing requests also need
//! `require_same_site` (CSRF).

mod apply_request;
mod audit;
mod channels;
mod csrf;
mod error;
mod hosts;
mod promote;
mod snapshot;

use std::sync::Arc;

use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Router, middleware};

pub use csrf::LODESTAR_HEADER;

use crate::access::{AccessState, require_access};
use crate::state::AppState;
use crate::{embed, websocket};
use error::ApiError;

/// The complete dashboard app. Serve it with
/// `into_make_service_with_connect_info::<SocketAddr>()` (access control needs the peer).
pub fn app(state: AppState) -> Router {
    let access = AccessState {
        provider: state.online.clone(),
        config: Arc::new(state.config.access.clone()),
    };
    let api = Router::new()
        .route("/snapshot", get(snapshot::get_snapshot))
        .route("/hosts/{name}/apply", post(hosts::apply_host))
        .route("/channels/{channel}/apply", post(channels::apply_channel))
        .route("/promote", post(promote::promote))
        .route("/hosts/{name}/applies/{id}/log", get(hosts::apply_log))
        .route("/audit", get(audit::audit))
        .route("/ws", get(websocket::handler))
        .fallback(|| async { ApiError(StatusCode::NOT_FOUND, "not found".into()) })
        .layer(middleware::from_fn(csrf::require_same_site));
    Router::new()
        .nest("/api", api)
        .fallback(embed::static_handler)
        .layer(middleware::from_fn_with_state(access, require_access))
        .with_state(state)
}
