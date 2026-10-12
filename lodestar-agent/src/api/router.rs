use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::middleware;
use axum::routing::{get, post};

use super::{AppState, activations, apply, log, source_check, status};

const BODY_LIMIT: usize = 4096;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/status", get(status::get_status))
        .route("/apply", post(apply::post_apply))
        .route("/apply/log", get(log::get_log))
        .route("/activations", get(activations::get_activations))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            source_check::check_source,
        ))
        .with_state(state)
}
