//! `POST /apply`.

use std::time::Instant;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use lodestar_proto::{ApplyRequest, ApplyState, is_commit_hash};
use tokio::sync::OwnedMutexGuard;

use super::app_state::{AppState, Pending, live_pending};
use super::error::internal;
use crate::apply::{is_valid_apply_id, latest_record, read_record};
use crate::launcher;

pub(super) async fn post_apply(
    State(state): State<AppState>,
    Json(request): Json<ApplyRequest>,
) -> Response {
    if !is_commit_hash(&request.commit) || !is_valid_apply_id(&request.apply_id) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let guard = state.apply_lock.clone().lock_owned().await;
    let result = tokio::task::spawn_blocking(move || launch_apply(state, guard, request)).await;
    result.unwrap_or_else(internal)
}

fn launch_apply(
    state: AppState,
    mut guard: OwnedMutexGuard<Option<Pending>>,
    request: ApplyRequest,
) -> Response {
    let pending = live_pending(&state, &mut guard);
    if let Some(record) = read_record(&state.config, &request.apply_id) {
        return (StatusCode::OK, Json(record)).into_response();
    }
    if let Some(id) = pending {
        return if id == request.apply_id {
            StatusCode::ACCEPTED.into_response()
        } else {
            StatusCode::CONFLICT.into_response()
        };
    }
    if latest_record(&state.config).is_some_and(|record| record.state == ApplyState::Running) {
        return StatusCode::CONFLICT.into_response();
    }
    match launcher::launch(
        &state.config,
        &state.config_path,
        &request.apply_id,
        &request.commit,
    ) {
        Ok(()) => {
            *guard = Some((request.apply_id, Instant::now()));
            StatusCode::ACCEPTED.into_response()
        }
        Err(error) => internal(format!("launching apply: {error:#}")),
    }
}
