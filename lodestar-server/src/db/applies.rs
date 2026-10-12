//! The `applies` table: every apply the server requested.

use anyhow::Context;
use lodestar_proto::ApplyRecord;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ConnectionTrait, EntityTrait};
use serde::Serialize;

use super::entities::applies;

/// snake_case serde string of a unit-variant enum.
fn enum_str<T: Serialize>(value: &T) -> anyhow::Result<String> {
    match serde_json::to_value(value)? {
        serde_json::Value::String(text) => Ok(text),
        other => anyhow::bail!("expected a string enum, got {other}"),
    }
}

pub async fn insert_apply(
    db: &impl ConnectionTrait,
    host: &str,
    requested_by: &str,
    requested_at: i64,
    record: &ApplyRecord,
) -> anyhow::Result<()> {
    applies::ActiveModel {
        apply_id: Set(record.apply_id.clone()),
        host: Set(host.to_string()),
        commit: Set(record.commit.clone()),
        requested_by: Set(requested_by.to_string()),
        requested_at: Set(requested_at),
        state: Set(enum_str(&record.state)?),
        skip_reason: Set(record.skip_reason.as_ref().map(enum_str).transpose()?),
        message: Set(record.message.clone()),
        exit_code: Set(record.exit_code),
        started_at: Set(record.started_at),
        finished_at: Set(record.finished_at),
        log: Set(None),
    }
    .insert(db)
    .await?;
    Ok(())
}

/// Updates state fields of an existing apply from an agent's record. Leaves host,
/// requester and log untouched. Errors if the apply is unknown.
pub async fn update_apply_from_record(
    db: &impl ConnectionTrait,
    record: &ApplyRecord,
) -> anyhow::Result<()> {
    let existing = applies::Entity::find_by_id(record.apply_id.clone())
        .one(db)
        .await?
        .with_context(|| format!("unknown apply {}", record.apply_id))?;
    let mut active: applies::ActiveModel = existing.into();
    active.state = Set(enum_str(&record.state)?);
    active.skip_reason = Set(record.skip_reason.as_ref().map(enum_str).transpose()?);
    active.message = Set(record.message.clone());
    active.exit_code = Set(record.exit_code);
    active.started_at = Set(record.started_at);
    active.finished_at = Set(record.finished_at);
    active.update(db).await?;
    Ok(())
}

pub async fn get_apply(
    db: &impl ConnectionTrait,
    apply_id: &str,
) -> anyhow::Result<Option<applies::Model>> {
    Ok(applies::Entity::find_by_id(apply_id.to_string())
        .one(db)
        .await?)
}

/// Stores the server's copy of an apply's full log. Errors if the apply is unknown.
pub async fn set_apply_log(
    db: &impl ConnectionTrait,
    apply_id: &str,
    log: &str,
) -> anyhow::Result<()> {
    let existing = get_apply(db, apply_id)
        .await?
        .with_context(|| format!("unknown apply {apply_id}"))?;
    let mut active: applies::ActiveModel = existing.into();
    active.log = Set(Some(log.to_string()));
    active.update(db).await?;
    Ok(())
}
