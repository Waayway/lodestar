use std::sync::Arc;

use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Extension, Router, middleware};

use super::error::ApiError;
use super::{
    audit, builds, channels, csrf, hosts, me, metrics, promote, public_services, security_headers,
    snapshot, updates,
};
use crate::access::{
    AccessState, HostAllowlist, Need, OriginPolicy, load_viewer, require_access,
    require_known_host, require_need, require_origin, require_public_origin, require_trusted_peer,
};
use crate::auth::AuthState;
use crate::state::AppState;
use crate::{auth, embed, websocket};

/// The complete dashboard app on the tailnet listener: only requests naming this server
/// (listen address, `allowed_hosts`) get to the access check; a browser's `Origin` must be
/// the request's own. Serve it with `into_make_service_with_connect_info::<SocketAddr>()`
/// (access control needs the peer).
pub fn app(state: AppState) -> Router {
    let access = AccessState {
        provider: state.online.clone(),
        config: Arc::new(state.config.access.clone()),
    };
    let hosts = Arc::new(HostAllowlist::new(
        state.config.listen,
        &state.config.allowed_hosts,
    ));
    let websocket =
        Router::new()
            .route("/ws", get(websocket::handler))
            .layer(middleware::from_fn_with_state(
                OriginPolicy::SameAsHost,
                require_origin,
            ));
    Router::new()
        .nest("/api", api_routes(websocket, OriginPolicy::SameAsHost))
        .route("/metrics", get(not_found))
        .fallback(embed::static_handler)
        .layer(middleware::from_fn_with_state(access, require_access))
        .layer(middleware::from_fn_with_state(hosts, require_known_host))
        .layer(middleware::from_fn(security_headers::harden))
        .with_state(state)
}

/// The dashboard app on the router entrance: only trusted proxies get in; the viewer comes
/// from the session cookie and the router's tailnet mark; the WebSocket also needs a logged-in
/// viewer and the public URL as its `Origin`, as does every state-changing request that sends
/// one; `/auth` logs people in and out. `Host` is the router's to check: only trusted proxies
/// reach this listener. Serve it like [`app`].
///
/// Panics when `state.auth` is `None` (the server sets it whenever a router entrance is
/// configured).
pub fn proxy_app(state: AppState) -> Router {
    let auth = state
        .auth
        .clone()
        .expect("the router entrance needs AppState.auth");
    let origin = OriginPolicy::public(&auth.config);
    let websocket = Router::new()
        .route("/ws", get(websocket::handler))
        .layer(middleware::from_fn_with_state(
            auth.clone(),
            require_public_origin,
        ))
        .layer(middleware::from_fn(|request, next| {
            require_need(Need::Read, request, next)
        }));
    Router::new()
        .nest("/api", api_routes(websocket, origin.clone()))
        .nest("/auth", auth_routes(auth.clone(), origin))
        .route("/metrics", get(not_found))
        .fallback(embed::static_handler)
        .layer(middleware::from_fn_with_state(state.clone(), load_viewer))
        .layer(middleware::from_fn_with_state(auth, require_trusted_peer))
        .layer(middleware::from_fn(security_headers::harden))
        .with_state(state)
}

/// The Prometheus listener: only `GET /metrics`, no tailnet access check (the firewall
/// restricts who can reach the port).
pub fn metrics_app(state: AppState) -> Router {
    Router::new()
        .route("/metrics", get(metrics::get_metrics))
        .with_state(state)
}

/// Login, its callback, and logout (state-changing, so behind the CSRF guard).
fn auth_routes(auth: Arc<AuthState>, origin: OriginPolicy) -> Router<AppState> {
    Router::new()
        .route("/login", get(auth::login))
        .route("/callback", get(auth::callback))
        .route(
            "/logout",
            post(auth::logout).layer(middleware::from_fn_with_state(
                origin,
                csrf::require_same_site,
            )),
        )
        .fallback(not_found)
        .layer(Extension(auth))
}

/// Everything under `/api`: the route groups, the given WebSocket route, a JSON 404 for
/// unknown paths, and the CSRF guard with the listener's `origin` policy.
fn api_routes(websocket: Router<AppState>, origin: OriginPolicy) -> Router<AppState> {
    Router::new()
        .merge(public_api_routes())
        .merge(read_routes())
        .merge(act_routes())
        .merge(websocket)
        .fallback(not_found)
        .layer(middleware::from_fn_with_state(
            origin,
            csrf::require_same_site,
        ))
}

async fn not_found() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "not found".into())
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
        .route("/updates", get(updates::get_updates))
        .route("/builds", get(builds::get_builds))
        .route("/builds/recent", get(builds::recent_builds))
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
        .route("/updates/run", post(updates::run_updates))
        .route("/builds/{commit}/retry", post(builds::retry_build))
        .layer(middleware::from_fn(|request, next| {
            require_need(Need::Act, request, next)
        }))
}
