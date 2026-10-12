//! `GET /activations`.

use axum::Json;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use super::AppState;
use super::error::internal;
use crate::activations;

#[derive(Deserialize)]
pub(super) struct SinceQuery {
    #[serde(default)]
    since: u64,
}

pub(super) async fn get_activations(
    State(state): State<AppState>,
    Query(query): Query<SinceQuery>,
) -> Response {
    let path = state.config.state_dir.join("activations.jsonl");
    match tokio::task::spawn_blocking(move || activations::read_since(&path, query.since)).await {
        Ok(response) => Json(response).into_response(),
        Err(error) => internal(error),
    }
}
