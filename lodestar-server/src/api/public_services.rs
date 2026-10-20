//! `GET /api/public/services`.

use axum::Json;
use axum::extract::State;
use axum::http::header;
use axum::response::IntoResponse;

use crate::state::{AppState, public_services};

/// The reduced service list; `no-store`, like `/api/me`, so it is always current.
pub(super) async fn list(State(state): State<AppState>) -> impl IntoResponse {
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(public_services(&state)),
    )
}
