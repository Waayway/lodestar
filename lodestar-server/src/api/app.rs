use std::sync::Arc;

use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Router, middleware};

use super::error::ApiError;
use super::{audit, channels, csrf, hosts, me, promote, public_services, snapshot};
use crate::access::{AccessState, Need, require_access, require_need};
use crate::state::AppState;
use crate::{embed, websocket};

/// The complete dashboard app. Serve it with
/// `into_make_service_with_connect_info::<SocketAddr>()` (access control needs the peer).
pub fn app(state: AppState) -> Router {
    let access = AccessState {
        provider: state.online.clone(),
        config: Arc::new(state.config.access.clone()),
    };
    let api = Router::new()
        .merge(public_api_routes())
        .merge(read_routes())
        .merge(act_routes())
        .route("/ws", get(websocket::handler))
        .fallback(|| async { ApiError(StatusCode::NOT_FOUND, "not found".into()) })
        .layer(middleware::from_fn(csrf::require_same_site));
    Router::new()
        .nest("/api", api)
        .fallback(embed::static_handler)
        .layer(middleware::from_fn_with_state(access, require_access))
        .with_state(state)
}

/// Routes anyone may call.
fn public_api_routes() -> Router<AppState> {
    Router::new()
        .route("/me", get(me::me))
        .route("/public/services", get(public_services::list))
        .layer(middleware::from_fn(|request, next| {
            require_need(Need::Public, request, next)
        }))
}

/// Routes that need a logged-in reader or admin.
fn read_routes() -> Router<AppState> {
    Router::new()
        .route("/snapshot", get(snapshot::get_snapshot))
        .route("/hosts/{name}/applies/{id}/log", get(hosts::apply_log))
        .route("/audit", get(audit::audit))
        .layer(middleware::from_fn(|request, next| {
            require_need(Need::Read, request, next)
        }))
}

/// Routes that change something: admins over Tailscale only.
fn act_routes() -> Router<AppState> {
    Router::new()
        .route("/hosts/{name}/apply", post(hosts::apply_host))
        .route("/channels/{channel}/apply", post(channels::apply_channel))
        .route("/promote", post(promote::promote))
        .layer(middleware::from_fn(|request, next| {
            require_need(Need::Act, request, next)
        }))
}
