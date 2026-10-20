//! `GET /api/me`.

use axum::Extension;
use axum::Json;
use axum::http::header;
use axum::response::IntoResponse;

use crate::access::Viewer;

/// Who the viewer is; `no-store`, so no cache in between hands one viewer's answer to another.
pub(super) async fn me(Extension(viewer): Extension<Viewer>) -> impl IntoResponse {
    ([(header::CACHE_CONTROL, "no-store")], Json(viewer.me()))
}
