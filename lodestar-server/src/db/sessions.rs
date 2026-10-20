//! The `sessions` table: login sessions keyed by the hash of the cookie value.

use anyhow::{Context, bail};
use lodestar_proto::Role;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
};

use super::entities::sessions;
use crate::auth::LoginSession;

pub async fn insert_session(
    db: &impl ConnectionTrait,
    session: &LoginSession,
    created_at: i64,
) -> anyhow::Result<()> {
    let role = match session.role {
        Role::Reader | Role::Admin => serde_json::to_value(session.role)?
            .as_str()
            .context("role is not a string")?
            .to_string(),
        Role::Anonymous => bail!("an anonymous visitor cannot have a session"),
    };
    sessions::ActiveModel {
        id_hash: Set(session.id_hash.clone()),
        user: Set(session.user.clone()),
        role: Set(role),
        created_at: Set(created_at),
        expires_at: Set(session.expires_at),
    }
    .insert(db)
    .await
    .context("inserting session")?;
    Ok(())
}

/// The live session for `id_hash`; an expired row is deleted and gives `None`.
pub async fn find_session(
    db: &impl ConnectionTrait,
    id_hash: &str,
    now: i64,
) -> anyhow::Result<Option<LoginSession>> {
    let Some(row) = sessions::Entity::find_by_id(id_hash.to_string())
        .one(db)
        .await
        .context("finding session")?
    else {
        return Ok(None);
    };
    if row.expires_at <= now {
        delete_session(db, id_hash).await?;
        return Ok(None);
    }
    let role: Role = serde_json::from_value(serde_json::Value::String(row.role.clone()))
        .with_context(|| format!("unknown session role {:?}", row.role))?;
    Ok(Some(LoginSession {
        id_hash: row.id_hash,
        user: row.user,
        role,
        expires_at: row.expires_at,
    }))
}

pub async fn delete_session(db: &impl ConnectionTrait, id_hash: &str) -> anyhow::Result<()> {
    sessions::Entity::delete_by_id(id_hash.to_string())
        .exec(db)
        .await
        .context("deleting session")?;
    Ok(())
}

/// Removes every session that expired at or before `now`; returns how many.
pub async fn delete_expired_sessions(db: &impl ConnectionTrait, now: i64) -> anyhow::Result<u64> {
    let result = sessions::Entity::delete_many()
        .filter(sessions::Column::ExpiresAt.lte(now))
        .exec(db)
        .await
        .context("deleting expired sessions")?;
    Ok(result.rows_affected)
}
