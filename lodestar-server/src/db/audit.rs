//! The `audit_events` table.

use lodestar_proto::AuditEvent;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect,
};

use super::entities::audit_events;

pub async fn insert_audit(
    db: &impl ConnectionTrait,
    time: i64,
    host: Option<&str>,
    kind: &str,
    actor: &str,
    details: serde_json::Value,
) -> anyhow::Result<()> {
    audit_events::ActiveModel {
        id: sea_orm::NotSet,
        time: Set(time),
        host: Set(host.map(str::to_string)),
        kind: Set(kind.to_string()),
        actor: Set(actor.to_string()),
        details: Set(details),
    }
    .insert(db)
    .await?;
    Ok(())
}

/// Newest first (by time, then id).
pub async fn recent_audit(
    db: &impl ConnectionTrait,
    limit: u64,
) -> anyhow::Result<Vec<AuditEvent>> {
    let rows = audit_events::Entity::find()
        .order_by_desc(audit_events::Column::Time)
        .order_by_desc(audit_events::Column::Id)
        .limit(limit)
        .all(db)
        .await?;
    Ok(rows.into_iter().map(audit_from_row).collect())
}

fn audit_from_row(row: audit_events::Model) -> AuditEvent {
    AuditEvent {
        id: row.id,
        time: row.time,
        host: row.host,
        kind: row.kind,
        actor: row.actor,
        details: row.details,
    }
}

/// Newest audit event of `kind` for `host`.
pub async fn last_audit(
    db: &impl ConnectionTrait,
    host: &str,
    kind: &str,
) -> anyhow::Result<Option<AuditEvent>> {
    let row = audit_events::Entity::find()
        .filter(audit_events::Column::Host.eq(host))
        .filter(audit_events::Column::Kind.eq(kind))
        .order_by_desc(audit_events::Column::Time)
        .order_by_desc(audit_events::Column::Id)
        .one(db)
        .await?;
    Ok(row.map(audit_from_row))
}
