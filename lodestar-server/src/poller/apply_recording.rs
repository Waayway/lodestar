//! Mirrors the agent's apply record into the server's `applies` row.

use lodestar_proto::{ApplyRecord, ApplyState};
use sea_orm::TransactionTrait;

use super::log_cap::read_log_capped;
use super::log_follower::follow_log;
use crate::clock::now;
use crate::config::ManagedHost;
use crate::db;
use crate::state::AppState;

fn state_str(state: &ApplyState) -> &'static str {
    match state {
        ApplyState::Idle => "idle",
        ApplyState::Running => "running",
        ApplyState::Ok => "ok",
        ApplyState::Skipped => "skipped",
        ApplyState::Failed => "failed",
    }
}

fn is_terminal(state: &str) -> bool {
    matches!(state, "ok" | "skipped" | "failed")
}

/// Mirrors the agent's latest apply record into the server's copy. On the first terminal
/// state: audit `apply_finished` and store the full log. The log is fetched before any row
/// is touched, and the row update, audit row and log are written in one transaction, so a
/// failure leaves the row non-terminal and the next poll retries. Applies the server didn't
/// create are ignored. A running apply gets a log follower.
pub(super) async fn record_apply(
    state: &AppState,
    host: &ManagedHost,
    base: &str,
    record: &ApplyRecord,
) -> anyhow::Result<()> {
    if record.state == ApplyState::Running {
        let (state, host, id) = (state.clone(), host.clone(), record.apply_id.clone());
        tokio::spawn(async move { follow_log(&state, &host, &id).await });
    } else {
        // The apply ended: a later run with this id (none expected) may be followed again.
        state
            .capped_logs
            .lock()
            .unwrap()
            .remove(&(host.name.clone(), record.apply_id.clone()));
    }
    let Some(row) = db::get_apply(&state.db, &record.apply_id).await? else {
        return Ok(());
    };
    if row.host != host.name || is_terminal(&row.state) {
        return Ok(());
    }
    let new_state = state_str(&record.state);
    if row.state == new_state {
        return Ok(());
    }
    if !is_terminal(new_state) {
        return db::update_apply_from_record(&state.db, record).await;
    }
    let log = read_log_capped(state, base, &record.apply_id).await?;
    let details = serde_json::to_value(record)?;
    let transaction = state.db.begin().await?;
    db::update_apply_from_record(&transaction, record).await?;
    db::insert_audit(
        &transaction,
        record.finished_at.unwrap_or_else(now),
        Some(&host.name),
        "apply_finished",
        "agent",
        details,
    )
    .await?;
    db::set_apply_log(&transaction, &record.apply_id, &log).await?;
    transaction.commit().await?;
    Ok(())
}
