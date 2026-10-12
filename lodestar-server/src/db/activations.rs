//! The activations cursor in `host_state`: the next activation line to ingest.

use sea_orm::sea_query::OnConflict;
use sea_orm::{ActiveValue::Set, ConnectionTrait, EntityTrait};

use super::entities::host_state;

pub async fn activations_next(db: &impl ConnectionTrait, host: &str) -> anyhow::Result<i64> {
    Ok(host_state::Entity::find_by_id(host.to_string())
        .one(db)
        .await?
        .map_or(0, |row| row.activations_next))
}

pub async fn set_activations_next(
    db: &impl ConnectionTrait,
    host: &str,
    next: i64,
) -> anyhow::Result<()> {
    host_state::Entity::insert(host_state::ActiveModel {
        name: Set(host.to_string()),
        last_status: Set(None),
        last_ok_at: Set(None),
        activations_next: Set(next),
    })
    .on_conflict(
        OnConflict::column(host_state::Column::Name)
            .update_column(host_state::Column::ActivationsNext)
            .to_owned(),
    )
    .exec(db)
    .await?;
    Ok(())
}
