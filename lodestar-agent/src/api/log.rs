//! `GET /apply/log`.

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use super::AppState;
use super::error::internal;
use crate::apply::read_log;

#[derive(Deserialize)]
pub(super) struct LogQuery {
    apply_id: String,
    #[serde(default)]
    offset: u64,
}

pub(super) async fn get_log(
    State(state): State<AppState>,
    Query(query): Query<LogQuery>,
) -> Response {
    let result =
        tokio::task::spawn_blocking(move || read_log(&state.config, &query.apply_id, query.offset))
            .await;
    match result {
        Ok(Some(chunk)) => Json(chunk).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => internal(error),
    }
}
