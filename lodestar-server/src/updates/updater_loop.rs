use std::time::Duration;

use super::updater::{restore_last, run_all};
use crate::state::AppState;

/// Restores the last result per input, then runs the updater every `interval_secs`; the
/// first run is one interval after start, not at boot.
pub fn spawn(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        restore_last(&state).await;
        let interval = Duration::from_secs(state.config.updates.interval_secs.max(1));
        loop {
            tokio::time::sleep(interval).await;
            if let Err(error) = run_all(&state).await {
                eprintln!("lodestar-server: scheduled update skipped: {error}");
            }
        }
    })
}
