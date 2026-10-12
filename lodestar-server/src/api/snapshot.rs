//! `GET /api/snapshot`.

use axum::extract::State;
use axum::{Extension, Json};
use lodestar_proto::Snapshot;

use crate::access::Actor;
use crate::state::{AppState, snapshot};

pub(super) async fn get_snapshot(
    State(state): State<AppState>,
    Extension(Actor(viewer)): Extension<Actor>,
) -> Json<Snapshot> {
    Json(snapshot(&state, viewer).await)
}
