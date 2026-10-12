//! `POST /api/promote`.

use axum::extract::State;
use axum::http::StatusCode;
use axum::{Extension, Json};
use lodestar_proto::WsMessage;
use serde::Deserialize;
use serde_json::{Value, json};

use super::apply_request::fetch_mirror;
use super::error::ApiError;
use crate::access::Actor;
use crate::clock::now;
use crate::db;
use crate::mirror::PromoteError;
use crate::state::{AppState, channels};

#[derive(Deserialize)]
pub(super) struct PromoteBody {
    commit: String,
}

pub(super) async fn promote(
    State(state): State<AppState>,
    Extension(Actor(actor)): Extension<Actor>,
    Json(body): Json<PromoteBody>,
) -> Result<Json<Value>, ApiError> {
    if !lodestar_proto::is_commit_hash(&body.commit) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "commit must be 40 lowercase hex characters".into(),
        ));
    }
    let action = state.actions.lock().await;
    fetch_mirror(&state).await?;
    let Some(target) = state
        .mirror
        .promote_channels()
        .map(|channels| channels.to.clone())
    else {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            "promote is disabled on this server".into(),
        ));
    };
    let previous = state.mirror.head(&target).await;
    state
        .mirror
        .promote(&body.commit)
        .await
        .map_err(|error| match error {
            PromoteError::Rejected(message) => ApiError(StatusCode::CONFLICT, message),
            PromoteError::Git(error) => {
                eprintln!("lodestar-server: api: promote: {error:#}");
                ApiError(
                    StatusCode::BAD_GATEWAY,
                    format!("promote failed: {error:#}"),
                )
            }
        })?;
    // The push happened; a failure to record it must not turn the answer into an error.
    let details = json!({ "commit": body.commit, "previous": previous, "channel": target });
    if let Err(error) = db::insert_audit(&state.db, now(), None, "promote", &actor, details).await {
        eprintln!("lodestar-server: api: auditing promote: {error:#}");
    }
    if let Err(error) = state.mirror.fetch().await {
        eprintln!("lodestar-server: api: mirror fetch after promote: {error:#}");
    }
    drop(action);
    state.broadcast(WsMessage::ChannelsUpdated(channels(&state).await));
    Ok(Json(json!({ "commit": body.commit })))
}
