//! `GET /api/updates` and `POST /api/updates/run`.

use axum::extract::State;
use axum::http::StatusCode;
use axum::{Extension, Json};
use lodestar_proto::{InputView, SoftwareView, UpdateRunView, UpdatesView, VersionChangeView};
use serde_json::{Value, json};

use super::error::ApiError;
use crate::access::Actor;
use crate::clock::now;
use crate::db;
use crate::state::AppState;
use crate::updates::update_run::{UpdateOutcome, UpdateRun};
use crate::updates::updater::run_all_locked;

fn run_view(run: &UpdateRun) -> UpdateRunView {
    let (commit, changes, host) = match &run.outcome {
        UpdateOutcome::Pushed { commit, changes } => (Some(commit.clone()), changes.clone(), None),
        UpdateOutcome::EvalFailed { host, .. } | UpdateOutcome::BuildFailed { host, .. }
            if !host.is_empty() =>
        {
            (None, Vec::new(), Some(host.clone()))
        }
        _ => (None, Vec::new(), None),
    };
    UpdateRunView {
        finished_at: run.finished_at,
        result: run.outcome.result().to_string(),
        commit,
        changes: changes
            .into_iter()
            .map(|change| VersionChangeView {
                software: change.software,
                from: change.from,
                to: change.to,
            })
            .collect(),
        host,
    }
}

pub(super) async fn get_updates(State(state): State<AppState>) -> Json<UpdatesView> {
    let latest = state.latest.lock().unwrap().clone();
    let software = state
        .config
        .updates
        .software
        .iter()
        .map(|(name, config)| {
            let known = latest.get(name).cloned().unwrap_or_default();
            SoftwareView {
                name: name.clone(),
                input: config.input.clone(),
                latest: known.version,
                checked_at: known.checked_at,
                error: known.error,
            }
        })
        .collect();
    let last = state.updates.last.lock().unwrap().clone();
    let inputs = state
        .config
        .updates
        .inputs()
        .into_keys()
        .map(|name| InputView {
            last: last.get(&name).map(run_view),
            name,
        })
        .collect();
    // A flag, not a lock probe: a poll must never make a concurrent start fail.
    let running = state.updates.is_running();
    Json(UpdatesView {
        software,
        inputs,
        running,
    })
}

pub(super) async fn run_updates(
    State(state): State<AppState>,
    Extension(Actor(actor)): Extension<Actor>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    // Take the run lock before answering: a status read right after the 202 then sees
    // `running`, and two requests cannot both start a run.
    let Ok(running) = state.updates.try_start() else {
        return Err(ApiError(StatusCode::CONFLICT, "already running".into()));
    };
    if let Err(error) = db::insert_audit(
        &state.db,
        now(),
        None,
        "update-requested",
        &actor,
        json!({}),
    )
    .await
    {
        eprintln!("lodestar-server: api: auditing update request: {error:#}");
    }
    tokio::spawn(async move {
        run_all_locked(&state, running).await;
    });
    Ok((StatusCode::ACCEPTED, Json(json!({}))))
}
