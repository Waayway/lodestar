//! `POST /api/hosts/{name}/apply` and `GET /api/hosts/{name}/applies/{id}/log`.

use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde_json::{Value, json};

use super::apply_request::{find_host, request_applies, send_apply};
use super::error::{ApiError, internal};
use crate::access::Actor;
use crate::db;
use crate::poller::agent_base;
use crate::state::AppState;

pub(super) async fn apply_host(
    State(state): State<AppState>,
    Extension(Actor(actor)): Extension<Actor>,
    Path(name): Path<String>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let host = find_host(&state, &name)?;
    let (commit, ids) = request_applies(&state, &actor, &[host], &host.channel).await?;
    let apply_id = &ids[0];
    let agent_status = send_apply(&state, host, apply_id, &commit).await;
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "apply_id": apply_id, "agent_status": agent_status })),
    ))
}

/// The server's stored copy of an apply's log, or else the agent's live log from the start.
pub(super) async fn apply_log(
    State(state): State<AppState>,
    Path((name, id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let host = find_host(&state, &name)?;
    let row = db::get_apply(&state.db, &id)
        .await
        .map_err(internal)?
        .filter(|row| row.host == host.name)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("unknown apply {id}")))?;
    let log = match row.log {
        Some(log) => log,
        None => {
            let peers = state.online.peers().await;
            let base = agent_base(&state.config, host, &peers).ok_or_else(|| {
                ApiError(
                    StatusCode::SERVICE_UNAVAILABLE,
                    format!("{} is offline", host.name),
                )
            })?;
            state
                .client
                .log(&base, &id, 0)
                .await
                .map_err(|error| ApiError(StatusCode::BAD_GATEWAY, format!("{error:#}")))?
                .data
        }
    };
    Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], log).into_response())
}
