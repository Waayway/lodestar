//! The `desired_applies` table: the apply each host should run.

use sea_orm::sea_query::OnConflict;
use sea_orm::{ActiveValue::Set, ConnectionTrait, EntityTrait};

use super::entities::{applies, desired_applies};

pub async fn set_desired(
    db: &impl ConnectionTrait,
    host: &str,
    apply_id: &str,
) -> anyhow::Result<()> {
    desired_applies::Entity::insert(desired_applies::ActiveModel {
        host: Set(host.to_string()),
        apply_id: Set(apply_id.to_string()),
    })
    .on_conflict(
        OnConflict::column(desired_applies::Column::Host)
            .update_column(desired_applies::Column::ApplyId)
            .to_owned(),
    )
    .exec(db)
    .await?;
    Ok(())
}

pub async fn desired(
    db: &impl ConnectionTrait,
    host: &str,
) -> anyhow::Result<Option<applies::Model>> {
    let Some(desired) = desired_applies::Entity::find_by_id(host.to_string())
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    Ok(applies::Entity::find_by_id(desired.apply_id)
        .one(db)
        .await?)
}
