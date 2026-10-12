//! `GET /status`.

use axum::Json;
use axum::extract::State;
use axum::response::{IntoResponse, Response};

use super::AppState;
use super::error::internal;
use crate::status;

pub(super) async fn get_status(State(state): State<AppState>) -> Response {
    match tokio::task::spawn_blocking(move || status::collect(&state.config, &state.git)).await {
        Ok(agent_status) => Json(agent_status).into_response(),
        Err(error) => internal(error),
    }
}
