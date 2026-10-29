use lodestar_server::updates::updater_loop::next_run_at;
use lodestar_server::updates::updates_state::{AlreadyRunning, UpdatesState};

const DAY: u64 = 86_400;

#[test]
fn fresh_server_waits_one_interval() {
    assert_eq!(next_run_at(None, 1_000, DAY), 1_000 + 86_400);
}

#[test]
fn restart_keeps_the_schedule_of_the_last_run() {
    // Last run an hour ago: the next one is 23 hours from now, not a full day.
    assert_eq!(next_run_at(Some(10_000), 13_600, DAY), 10_000 + 86_400);
}

#[test]
fn overdue_run_happens_now() {
    // Restarts kept postponing it: run at once.
    assert_eq!(
        next_run_at(Some(10_000), 10_000 + 3 * 86_400, DAY),
        10_000 + 3 * 86_400
    );
}

#[test]
fn zero_interval_counts_as_one_second() {
    assert_eq!(next_run_at(Some(50), 50, 0), 51);
}

#[test]
fn run_guard_sets_and_clears_running() {
    let state = UpdatesState::default();
    assert!(!state.is_running());
    let guard = state.try_start().expect("free");
    assert!(state.is_running());
    assert_eq!(state.try_start().err(), Some(AlreadyRunning));
    // A status read does not touch the lock.
    assert!(state.is_running());
    drop(guard);
    assert!(!state.is_running());
    assert!(state.try_start().is_ok());
}

#[tokio::test]
async fn run_guard_clears_running_on_panic() {
    let state = std::sync::Arc::new(UpdatesState::default());
    let guard = state.try_start().expect("free");
    let task = tokio::spawn(async move {
        let _guard = guard;
        panic!("run failed");
    });
    assert!(task.await.is_err());
    assert!(!state.is_running());
    assert!(state.try_start().is_ok());
}
