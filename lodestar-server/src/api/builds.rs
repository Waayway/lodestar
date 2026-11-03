//! `GET /api/builds`, `GET /api/builds/recent` and `POST /api/builds/{commit}/retry`.

use std::collections::{BTreeMap, BTreeSet};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use lodestar_proto::{BuildView, BuildsView, ChannelView, HostBuildView};
use serde_json::{Value, json};

use super::error::{ApiError, internal};
use crate::access::Actor;
use crate::builds::{BuildResult, spawn_prebuild};
use crate::clock::now;
use crate::db;
use crate::state::{AppState, channels};

/// Every channel with the build state of its head.
pub(super) async fn get_builds(State(state): State<AppState>) -> Json<Vec<ChannelView>> {
    Json(channels(&state).await)
}

/// How many builds `GET /api/builds/recent` lists.
const RECENT_BUILDS: usize = 20;

/// Whether the gate is on, and the newest builds with every host's result: the commits being
/// built now first, then recorded results by when they were last written.
pub(super) async fn recent_builds(State(state): State<AppState>) -> Json<BuildsView> {
    let mut building: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (channel, commit) in state
        .building
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
    {
        building
            .entry(commit.clone())
            .or_default()
            .push(channel.clone());
    }
    let mut cached_for: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (channel, heads) in state.builds.channel_heads() {
        for commit in std::iter::once(heads.current).chain(heads.previous) {
            cached_for.entry(commit).or_default().push(channel.clone());
        }
    }
    let channels: BTreeSet<&str> = state
        .hosts
        .iter()
        .map(|host| host.channel.as_str())
        .collect();
    let mut head_of: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for channel in channels {
        if let Some(head) = state.mirror.head(channel).await {
            head_of.entry(head).or_default().push(channel.to_string());
        }
    }

    let recorded = state.builds.recent(RECENT_BUILDS);
    let mut commits: Vec<String> = building.keys().cloned().collect();
    for result in &recorded {
        if !commits.contains(&result.commit) {
            commits.push(result.commit.clone());
        }
    }
    commits.truncate(RECENT_BUILDS);

    let mut builds = Vec::with_capacity(commits.len());
    for commit in commits {
        let result = recorded
            .iter()
            .find(|result| result.commit == commit)
            .cloned()
            .or_else(|| state.builds.read(&commit))
            .unwrap_or_else(|| BuildResult::empty(&commit));
        builds.push(BuildView {
            subject: state.mirror.subject(&commit).await,
            building: building.remove(&commit).unwrap_or_default(),
            head_of: head_of.remove(&commit).unwrap_or_default(),
            cached_for: cached_for.remove(&commit).unwrap_or_default(),
            finished_at: result.hosts.values().map(|host| host.built_at).max(),
            hosts: result
                .hosts
                .into_iter()
                .map(|(name, host)| HostBuildView {
                    name,
                    ok: host.ok,
                    path: host.path,
                    log: host.log_tail,
                    built_at: host.built_at,
                })
                .collect(),
            commit,
        });
    }
    Json(BuildsView {
        enabled: state.config.builds.enable,
        builds,
    })
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
