//! The simulated agent's HTTP API.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use lodestar_proto::{
    ActivationsResponse, AgentStatus, ApplyRecord, ApplyRequest, ApplyState, LogChunk,
    is_commit_hash,
};
use serde::Deserialize;
use tokio::task::JoinHandle;

use super::SimAgent;
use super::sim_agent::Apply;
use crate::clock::now;

const BODY_LIMIT: usize = 4096;
/// Applies kept, like the real agent.
const KEEP_APPLIES: usize = 50;

/// The real agent's apply id rule: `[A-Za-z0-9][A-Za-z0-9_-]{0,63}`.
fn is_valid_apply_id(id: &str) -> bool {
    id.bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && id.len() <= 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

async fn get_status(State(agent): State<Arc<SimAgent>>) -> Json<AgentStatus> {
    Json(agent.status())
}

async fn post_apply(
    State(agent): State<Arc<SimAgent>>,
    Json(request): Json<ApplyRequest>,
) -> Response {
    if !is_commit_hash(&request.commit) || !is_valid_apply_id(&request.apply_id) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    {
        let mut inner = agent.inner.lock().unwrap();
        if let Some(apply) = inner
            .applies
            .iter()
            .find(|apply| apply.record.apply_id == request.apply_id)
        {
            return (StatusCode::OK, Json(apply.record.clone())).into_response();
        }
        if inner
            .applies
            .back()
            .is_some_and(|apply| apply.record.state == ApplyState::Running)
        {
            return StatusCode::CONFLICT.into_response();
        }
        inner.applies.push_back(Apply {
            record: ApplyRecord {
                apply_id: request.apply_id.clone(),
                commit: request.commit.clone(),
                state: ApplyState::Running,
                skip_reason: None,
                message: None,
                exit_code: None,
                started_at: Some(now()),
                finished_at: None,
            },
            log: String::new(),
        });
        while inner.applies.len() > KEEP_APPLIES {
            inner.applies.pop_front();
        }
    }
    tokio::spawn(agent.clone().run_apply(request.apply_id, request.commit));
    StatusCode::ACCEPTED.into_response()
}

#[derive(Deserialize)]
struct LogQuery {
    apply_id: String,
    #[serde(default)]
    offset: u64,
}

async fn get_log(State(agent): State<Arc<SimAgent>>, Query(query): Query<LogQuery>) -> Response {
    let inner = agent.inner.lock().unwrap();
    let Some(apply) = inner
        .applies
        .iter()
        .find(|apply| apply.record.apply_id == query.apply_id)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let data = usize::try_from(query.offset)
        .ok()
        .and_then(|offset| apply.log.get(offset..))
        .unwrap_or_default()
        .to_string();
    let at_end = query.offset + data.len() as u64 >= apply.log.len() as u64;
    Json(LogChunk {
        apply_id: query.apply_id,
        offset: query.offset,
        next: query.offset + data.len() as u64,
        data,
        eof: at_end && apply.record.state != ApplyState::Running,
    })
    .into_response()
}

#[derive(Deserialize)]
struct SinceQuery {
    #[serde(default)]
    since: u64,
}

async fn get_activations(
    State(agent): State<Arc<SimAgent>>,
    Query(query): Query<SinceQuery>,
) -> Json<ActivationsResponse> {
    let inner = agent.inner.lock().unwrap();
    Json(ActivationsResponse {
        events: inner
            .activations
            .iter()
            .filter(|event| event.line >= query.since)
            .cloned()
            .collect(),
        next: inner.activations.len() as u64,
    })
}

pub fn router(agent: Arc<SimAgent>) -> Router {
    Router::new()
        .route("/status", get(get_status))
        .route("/apply", post(post_apply))
        .route("/apply/log", get(get_log))
        .route("/activations", get(get_activations))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .with_state(agent)
}

/// Serves a simulated agent on `127.0.0.1:0`; returns its address and the server task.
pub async fn serve(agent: Arc<SimAgent>) -> anyhow::Result<(SocketAddr, JoinHandle<()>)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .context("binding a simulated agent")?;
    let addr = listener.local_addr()?;
    let name = agent.name.clone();
    let task = tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, router(agent)).await {
            eprintln!("lodestar-server: demo agent {name}: {error}");
        }
    });
    Ok((addr, task))
}
