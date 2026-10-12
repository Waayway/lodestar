//! `GET /api/audit`.

use axum::Json;
use axum::extract::{Query, State};
use lodestar_proto::AuditEvent;
use serde::Deserialize;

use super::error::{ApiError, internal};
use crate::db;
use crate::state::AppState;

const AUDIT_DEFAULT_LIMIT: u64 = 100;
const AUDIT_MAX_LIMIT: u64 = 1000;

#[derive(Deserialize)]
pub(super) struct AuditQuery {
    limit: Option<u64>,
}

pub(super) async fn audit(
    State(state): State<AppState>,
    Query(query): Query<AuditQuery>,
) -> Result<Json<Vec<AuditEvent>>, ApiError> {
    let limit = query
        .limit
        .unwrap_or(AUDIT_DEFAULT_LIMIT)
        .clamp(1, AUDIT_MAX_LIMIT);
    db::recent_audit(&state.db, limit)
        .await
        .map(Json)
        .map_err(internal)
}
