//! `GET /api/builds` and `POST /api/builds/{commit}/retry`.

use std::collections::BTreeSet;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use lodestar_proto::ChannelView;
use serde_json::{Value, json};

use super::error::{ApiError, internal};
use crate::access::Actor;
use crate::builds::spawn_prebuild;
use crate::clock::now;
use crate::db;
use crate::state::{AppState, channels};

/// Every channel with the build state of its head.
pub(super) async fn get_builds(State(state): State<AppState>) -> Json<Vec<ChannelView>> {
    Json(channels(&state).await)
}

/// Removes the failed hosts from `commit`'s result; when the commit is a channel's head the
/// channel is rebuilt right away instead of at the next apply.
pub(super) async fn retry_build(
    State(state): State<AppState>,
    Extension(Actor(actor)): Extension<Actor>,
    Path(commit): Path<String>,
) -> Result<Json<Value>, ApiError> {
    if !lodestar_proto::is_commit_hash(&commit) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "commit must be 40 lowercase hex characters".into(),
        ));
    }
    if state.builds.read(&commit).is_none() {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            format!("no build result for {commit}"),
        ));
    }
    let hosts = state.builds.retry(&commit).await.map_err(internal)?;
    let details = json!({ "commit": commit, "hosts": hosts });
    if let Err(error) =
        db::insert_audit(&state.db, now(), None, "build-retry", &actor, details).await
    {
        eprintln!("lodestar-server: api: auditing build retry: {error:#}");
    }
    if !hosts.is_empty() {
        let names: BTreeSet<&str> = state
            .hosts
            .iter()
            .map(|host| host.channel.as_str())
            .collect();
        for channel in names {
            if state.mirror.head(channel).await.as_deref() == Some(commit.as_str()) {
                spawn_prebuild(&state, channel, &commit);
            }
        }
    }
    Ok(Json(json!({ "retried": hosts })))
}
