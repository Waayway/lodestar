use axum::Extension;
use axum::extract::State;
use axum::extract::ws::WebSocketUpgrade;
use axum::response::Response;

use super::session::run;
use crate::access::Viewer;
use crate::state::AppState;

pub async fn handler(
    State(state): State<AppState>,
    Extension(viewer): Extension<Viewer>,
    upgrade: WebSocketUpgrade,
) -> Response {
    upgrade.on_upgrade(move |socket| run(state, viewer, socket))
}
