//! The status columns of `host_state`: the agent's last status and when it was received.

use lodestar_proto::AgentStatus;
use sea_orm::sea_query::OnConflict;
use sea_orm::{ActiveValue::Set, ConnectionTrait, EntityTrait};

use super::entities::host_state;

/// Stores the latest agent status and marks the host as last seen OK at `now`.
pub async fn save_status(
    db: &impl ConnectionTrait,
    host: &str,
    status: &AgentStatus,
    now: i64,
) -> anyhow::Result<()> {
    host_state::Entity::insert(host_state::ActiveModel {
        name: Set(host.to_string()),
        last_status: Set(Some(serde_json::to_value(status)?)),
        last_ok_at: Set(Some(now)),
        activations_next: Set(0),
    })
    .on_conflict(
        OnConflict::column(host_state::Column::Name)
            .update_columns([host_state::Column::LastStatus, host_state::Column::LastOkAt])
            .to_owned(),
    )
    .exec(db)
    .await?;
    Ok(())
}

/// Last stored status and the time it was received, if any.
pub async fn load_status(
    db: &impl ConnectionTrait,
    host: &str,
) -> anyhow::Result<Option<(AgentStatus, Option<i64>)>> {
    let Some(row) = host_state::Entity::find_by_id(host.to_string())
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    match row.last_status {
        Some(status) => Ok(Some((serde_json::from_value(status)?, row.last_ok_at))),
        None => Ok(None),
    }
}
