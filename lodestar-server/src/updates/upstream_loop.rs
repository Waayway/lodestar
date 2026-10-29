use std::time::Duration;

use super::upstream::check_once;
use crate::state::AppState;

/// Checks upstream releases at start and then every `upstream_interval_secs`.
pub fn spawn(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let interval = Duration::from_secs(state.config.updates.upstream_interval_secs.max(1));
        loop {
            check_once(&state).await;
            tokio::time::sleep(interval).await;
        }
    })
}
