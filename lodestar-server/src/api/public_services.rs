//! `GET /api/public/services`.

use axum::Json;
use axum::extract::State;
use lodestar_proto::PublicService;

use crate::state::{AppState, public_services};

pub(super) async fn list(State(state): State<AppState>) -> Json<Vec<PublicService>> {
    Json(public_services(&state))
}
