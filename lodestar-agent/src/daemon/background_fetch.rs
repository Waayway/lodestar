use std::time::Duration;

use lodestar_proto::ApplyState;

use crate::api::{AppState, live_pending};
use crate::apply::{latest_record, record_fetch, try_lock_apply};

const FETCH_INTERVAL: Duration = Duration::from_secs(600);
/// The background fetch holds the apply lock, so it must not hang.
const FETCH_TIMEOUT: Duration = Duration::from_secs(120);

/// Every 600 s, unless an apply is running or starting. Holds the apply lock
/// throughout so it never overlaps with an apply's own fetch.
pub(super) async fn background_fetch(state: AppState) {
    loop {
        tokio::time::sleep(FETCH_INTERVAL).await;
        let mut guard = state.apply_lock.clone().lock_owned().await;
        let state = state.clone();
        let _ = tokio::task::spawn_blocking(move || {
            if live_pending(&state, &mut guard).is_some()
                || latest_record(&state.config)
                    .is_some_and(|record| record.state == ApplyState::Running)
            {
                return;
            }
            // The wrapper may be running without a record yet (or a daemon restart).
            let Some(_flock) = try_lock_apply(&state.config) else {
                return;
            };
            match state
                .git
                .fetch_within(&state.config.repo_url, FETCH_TIMEOUT)
            {
                Ok(()) => record_fetch(&state.config),
                Err(error) => eprintln!("lodestar-agent: background fetch failed: {error:#}"),
            }
        })
        .await;
    }
}
