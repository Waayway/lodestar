//! Shared by the host and channel apply routes: record new desired applies and send them
//! to the agents.

use axum::http::StatusCode;
use lodestar_proto::{ApplyRecord, ApplyRequest, ApplyState, WsMessage};
use sea_orm::TransactionTrait;
use serde_json::json;

use super::error::{ApiError, internal};
use crate::clock::now;
use crate::config::ManagedHost;
use crate::db;
use crate::poller::agent_base;
use crate::state::{AppState, host_view};

pub(super) fn find_host<'a>(state: &'a AppState, name: &str) -> Result<&'a ManagedHost, ApiError> {
    state
        .hosts
        .iter()
        .find(|host| host.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("unknown host {name}")))
}

pub(super) async fn fetch_mirror(state: &AppState) -> Result<(), ApiError> {
    state.mirror.fetch().await.map_err(|error| {
        eprintln!("lodestar-server: api: mirror fetch: {error:#}");
        ApiError(
            StatusCode::BAD_GATEWAY,
            format!("mirror fetch failed: {error:#}"),
        )
    })
}

async fn channel_head(state: &AppState, channel: &str) -> Result<String, ApiError> {
    state.mirror.head(channel).await.ok_or_else(|| {
        ApiError(
            StatusCode::CONFLICT,
            format!("channel {channel} has no head in the mirror"),
        )
    })
}

/// Creates one new apply of `commit` per host and makes it the host's desired apply, with an
/// `apply_requested` audit row each, all in one transaction. Returns the new ids.
async fn create_applies(
    state: &AppState,
    actor: &str,
    hosts: &[&ManagedHost],
    commit: &str,
) -> anyhow::Result<Vec<String>> {
    let requested_at = now();
    let transaction = state.db.begin().await?;
    let mut ids = Vec::with_capacity(hosts.len());
    for host in hosts {
        let record = ApplyRecord {
            apply_id: uuid::Uuid::new_v4().to_string(),
            commit: commit.to_string(),
            state: ApplyState::Idle,
            skip_reason: None,
            message: None,
            exit_code: None,
            started_at: None,
            finished_at: None,
        };
        db::insert_apply(&transaction, &host.name, actor, requested_at, &record).await?;
        db::set_desired(&transaction, &host.name, &record.apply_id).await?;
        let details = json!({
            "apply_id": record.apply_id,
            "commit": commit,
            "channel": host.channel,
        });
        db::insert_audit(
            &transaction,
            requested_at,
            Some(&host.name),
            "apply_requested",
            actor,
            details,
        )
        .await?;
        ids.push(record.apply_id);
    }
    transaction.commit().await?;
    for host in hosts {
        let view = host_view(state, host).await;
        state.broadcast(WsMessage::HostUpdated(Box::new(view)));
    }
    Ok(ids)
}

/// Sends the apply to the host's agent. Returns the agent's answer: `"202"` (started), `"200"`
/// (already ran), `"409"` (busy), `"unreachable"` (offline or no answer) or `"error"`. The
/// poller re-sends a desired apply the agent never received.
pub(super) async fn send_apply(
    state: &AppState,
    host: &ManagedHost,
    apply_id: &str,
    commit: &str,
) -> String {
    let peers = state.online.peers().await;
    let Some(base) = agent_base(&state.config, host, &peers) else {
        return "unreachable".into();
    };
    let request = ApplyRequest {
        apply_id: apply_id.to_string(),
        commit: commit.to_string(),
    };
    match state.client.apply(&base, &request).await {
        Ok(code) if matches!(code.as_u16(), 200 | 202 | 409) => code.as_u16().to_string(),
        Ok(code) => {
            eprintln!(
                "lodestar-server: {}: apply {apply_id}: agent answered {code}",
                host.name
            );
            "error".into()
        }
        Err(error) => {
            eprintln!(
                "lodestar-server: {}: apply {apply_id}: {error:#}",
                host.name
            );
            "unreachable".into()
        }
    }
}

/// Fetches the mirror, resolves `channel`'s head and records a new desired apply of it for
/// each host. Holds the action lock throughout, so of two overlapping presses the later one
/// always records the newer head. Returns the commit and the new apply ids.
pub(super) async fn request_applies(
    state: &AppState,
    actor: &str,
    hosts: &[&ManagedHost],
    channel: &str,
) -> Result<(String, Vec<String>), ApiError> {
    let _action = state.actions.lock().await;
    fetch_mirror(state).await?;
    let commit = channel_head(state, channel).await?;
    let ids = create_applies(state, actor, hosts, &commit)
        .await
        .map_err(internal)?;
    Ok((commit, ids))
}
