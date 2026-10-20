//! `GET /api/me`.

use axum::Extension;
use axum::Json;
use lodestar_proto::Me;

use crate::access::Viewer;

pub(super) async fn me(Extension(viewer): Extension<Viewer>) -> Json<Me> {
    Json(viewer.me())
}
