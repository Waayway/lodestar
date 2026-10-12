//! `POST /api/channels/{channel}/apply`.

use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use futures::future::join_all;
use serde_json::{Value, json};

use super::apply_request::{request_applies, send_apply};
use super::error::ApiError;
use crate::access::Actor;
use crate::config::ManagedHost;
use crate::state::AppState;

pub(super) async fn apply_channel(
    State(state): State<AppState>,
    Extension(Actor(actor)): Extension<Actor>,
    Path(channel): Path<String>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let hosts: Vec<&ManagedHost> = state
        .hosts
        .iter()
        .filter(|host| host.channel == channel)
        .collect();
    if hosts.is_empty() {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            format!("no host on channel {channel}"),
        ));
    }
    let (commit, ids) = request_applies(&state, &actor, &hosts, &channel).await?;
    let statuses = join_all(
        hosts
            .iter()
            .zip(&ids)
            .map(|(host, id)| send_apply(&state, host, id, &commit)),
    )
    .await;
    let mut apply_ids = BTreeMap::new();
    let mut agent_status = BTreeMap::new();
    for ((host, id), status) in hosts.iter().zip(ids).zip(statuses) {
        apply_ids.insert(host.name.clone(), id);
        agent_status.insert(host.name.clone(), status);
    }
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "apply_ids": apply_ids, "agent_status": agent_status })),
    ))
}
