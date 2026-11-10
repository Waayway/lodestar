//! One poll of one host: status, apply record, activations and a re-send when needed.

use std::collections::HashMap;

use futures::future::join_all;
use lodestar_proto::{AgentStatus, ApplyRequest, WsMessage};

use super::activation_ingest::ingest_activations;
use super::apply_recording::record_apply;
use super::reachability::{agent_base, reachability_change};
use crate::agent_client::AgentEndpoint;
use crate::builds::{build_channel, known_failures, skips_gate};
use crate::clock::now;
use crate::config::ManagedHost;
use crate::db;
use crate::resend::should_resend;
use crate::state::{AppState, host_view_with};
use crate::tailscale::PeerInfo;

/// Logs a failed step for `host` and returns the value of a successful one.
pub(super) fn log_err<T>(what: &str, host: &str, result: anyhow::Result<T>) -> Option<T> {
    result
        .map_err(|error| eprintln!("lodestar-server: {host}: {what}: {error:#}"))
        .ok()
}

/// Polls all hosts concurrently. Returns the agent reachability changes it logged.
pub async fn poll_once(state: &AppState) -> Vec<String> {
    let peers = state.online.peers().await;
    join_all(
        state
            .hosts
            .iter()
            .map(|host| poll_host(state, host, &peers)),
    )
    .await
    .into_iter()
    .flatten()
    .collect()
}

async fn poll_host(
    state: &AppState,
    host: &ManagedHost,
    peers: &HashMap<String, PeerInfo>,
) -> Option<String> {
    let mut change = None;
    let status = match agent_base(&state.config, host, peers) {
        Some(base) => {
            let result = state.client.status(&base).await;
            change = reachability_change(state, &host.name, result.as_ref().map(|_| ()));
            result.ok().map(|status| (base, status))
        }
        None => None,
    };
    if let Some(line) = &change {
        eprintln!("lodestar-server: {line}");
    }
    state
        .reachable
        .lock()
        .unwrap()
        .insert(host.name.clone(), status.is_some());
    if let Some((base, status)) = status {
        handle_status(state, host, &base, &status).await;
    }
    let view = host_view_with(state, host, peers).await;
    state.broadcast(WsMessage::HostUpdated(Box::new(view)));
    change
}

async fn handle_status(
    state: &AppState,
    host: &ManagedHost,
    base: &AgentEndpoint,
    status: &AgentStatus,
) {
    let name = host.name.as_str();
    state
        .versions
        .lock()
        .unwrap()
        .insert(name.to_string(), status.versions.clone());
    log_err(
        "saving status",
        name,
        db::save_status(&state.db, name, status, now()).await,
    );
    if let Some(record) = &status.apply {
        log_err(
            "recording apply",
            name,
            record_apply(state, host, base, record).await,
        );
    }
    log_err(
        "ingesting activations",
        name,
        ingest_activations(state, name, base).await,
    );
    let desired = log_err(
        "loading desired apply",
        name,
        db::desired(&state.db, name).await,
    )
    .flatten();
    let (id, commit) = match &desired {
        Some(desired) => (
            Some(desired.apply_id.as_str()),
            Some(desired.commit.as_str()),
        ),
        None => (None, None),
    };
    if should_resend(id, commit, status)
        && let Some(desired) = &desired
    {
        let request = ApplyRequest {
            apply_id: desired.apply_id.clone(),
            commit: desired.commit.clone(),
        };
        // A darwin host's re-send never waits on the gate.
        let known = if skips_gate(&[host]) {
            Some(Vec::new())
        } else {
            known_failures(state, &host.channel, &request.commit)
        };
        match known {
            Some(failed) if failed.is_empty() => resend(state, name, base, &request).await,
            Some(failed) => refuse_resend(state, name, &request, &failed),
            // A build that could not run (builder unreachable) is tried again after a backoff.
            None if state.build_backoff.ready(&host.channel, &request.commit) => {
                spawn_gated_resend(state, host, base, request)
            }
            None => {}
        }
    }
}

async fn resend(state: &AppState, name: &str, base: &AgentEndpoint, request: &ApplyRequest) {
    if let Some(code) = log_err(
        "re-sending desired apply",
        name,
        state.client.apply(base, request).await,
    ) {
        eprintln!(
            "lodestar-server: {name}: re-sent apply {} ({}): {code}",
            request.apply_id, request.commit
        );
    }
}

/// Logs (once per apply) that `request` is not re-sent because its build failed.
fn refuse_resend(state: &AppState, name: &str, request: &ApplyRequest, failed: &[String]) {
    let first = state
        .resend_refused
        .lock()
        .unwrap()
        .insert((name.to_string(), request.apply_id.clone()));
    if first {
        eprintln!(
            "lodestar-server: {name}: not re-sending apply {}: build of {} failed on the builder: {}",
            request.apply_id,
            request.commit,
            failed.join(", ")
        );
    }
}

/// Builds the desired apply's commit for the host's channel in the background (the poller never
/// waits for a build), then re-sends it when the build is good and it is still the desired apply.
/// At most one such task per apply.
fn spawn_gated_resend(
    state: &AppState,
    host: &ManagedHost,
    base: &AgentEndpoint,
    request: ApplyRequest,
) {
    let key = (host.name.clone(), request.apply_id.clone());
    if !state.resend_builds.lock().unwrap().insert(key.clone()) {
        return;
    }
    let state = state.clone();
    let (name, channel, base) = (host.name.clone(), host.channel.clone(), base.clone());
    tokio::spawn(async move {
        match build_channel(&state, &channel, &request.commit).await {
            Ok(failed) if failed.is_empty() => {
                let still_desired = log_err(
                    "loading desired apply",
                    &name,
                    db::desired(&state.db, &name).await,
                )
                .flatten()
                .is_some_and(|desired| desired.apply_id == request.apply_id);
                if still_desired {
                    resend(&state, &name, &base, &request).await;
                }
            }
            Ok(failed) => refuse_resend(&state, &name, &request, &failed),
            Err(error) => eprintln!(
                "lodestar-server: {name}: not re-sending apply {}: build of {} failed: {error:#}",
                request.apply_id, request.commit
            ),
        }
        state.resend_builds.lock().unwrap().remove(&key);
    });
}
