use std::time::Duration;

use super::updater::{restore_last, run_all};
use crate::clock;
use crate::state::AppState;

/// When the next scheduled run is due: one interval after `last` (the newest finished run),
/// at once when that is already past, and one interval from `now` when nothing ran yet (a
/// fresh server never runs at boot). A restart therefore keeps the schedule.
pub fn next_run_at(last: Option<i64>, now: i64, interval_secs: u64) -> i64 {
    let interval = i64::try_from(interval_secs.max(1)).unwrap_or(i64::MAX);
    match last {
        Some(last) => now.max(last.saturating_add(interval)),
        None => now.saturating_add(interval),
    }
}

/// The newest `finished_at` over the last runs (restored ones included).
pub fn newest_finished(state: &AppState) -> Option<i64> {
    let last = state.updates.last.lock().unwrap();
    last.values().map(|run| run.finished_at).max()
}

/// Restores the last result per input, then runs the updater every `interval_secs`, counted
/// from the newest finished run (a run started through the API counts too).
pub fn spawn(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        restore_last(&state).await;
        let interval = state.config.updates.interval_secs;
        // Also covers a start that lost to a run in progress, which records no new run yet.
        let mut last_attempt: Option<i64> = None;
        loop {
            let last = newest_finished(&state).max(last_attempt);
            let now = clock::now();
            let wait = next_run_at(last, now, interval) - now;
            tokio::time::sleep(Duration::from_secs(u64::try_from(wait).unwrap_or(0))).await;
            last_attempt = Some(clock::now());
            if let Err(error) = run_all(&state).await {
                eprintln!(
                    "lodestar-server: scheduled update not started ({error}); the run in progress counts"
                );
            }
        }
    })
}
