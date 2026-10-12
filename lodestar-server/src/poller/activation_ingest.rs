use sea_orm::TransactionTrait;

use crate::db;
use crate::state::AppState;

/// Copies new activation lines into the audit log (kind `activation`). Rows and the
/// cursor are written in one transaction, so a failure never duplicates rows.
pub(super) async fn ingest_activations(
    state: &AppState,
    host: &str,
    base: &str,
) -> anyhow::Result<()> {
    let since = db::activations_next(&state.db, host).await?;
    let response = state.client.activations(base, since.max(0) as u64).await?;
    let next = response.next as i64;
    if response.events.is_empty() && next == since {
        return Ok(());
    }
    let transaction = state.db.begin().await?;
    for event in &response.events {
        let actor = event
            .sudo_user
            .clone()
            .unwrap_or_else(|| event.trigger.clone());
        db::insert_audit(
            &transaction,
            event.time,
            Some(host),
            "activation",
            &actor,
            serde_json::to_value(event)?,
        )
        .await?;
    }
    db::set_activations_next(&transaction, host, next).await?;
    transaction.commit().await?;
    Ok(())
}
