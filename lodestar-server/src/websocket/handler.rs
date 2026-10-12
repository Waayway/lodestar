use axum::Extension;
use axum::extract::State;
use axum::extract::ws::WebSocketUpgrade;
use axum::response::Response;

use super::session::run;
use crate::access::Actor;
use crate::state::AppState;

pub async fn handler(
    State(state): State<AppState>,
    Extension(Actor(viewer)): Extension<Actor>,
    upgrade: WebSocketUpgrade,
) -> Response {
    upgrade.on_upgrade(move |socket| run(state, viewer, socket))
}
