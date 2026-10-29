//! The updater's shared state: one run at a time, and the last run per input.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::run_guard::RunGuard;
use super::update_run::UpdateRun;

#[derive(Default)]
pub struct UpdatesState {
    /// Held (as an owned guard inside [`RunGuard`]) for a whole run; a second one fails with
    /// `AlreadyRunning`.
    pub running: Arc<tokio::sync::Mutex<()>>,
    /// True while a [`RunGuard`] exists; what status reads look at, so they never touch the
    /// lock (a probe could make a concurrent start fail).
    pub(super) active: Arc<AtomicBool>,
    /// Last run per flake input (restored from the audit log at start).
    pub last: std::sync::Mutex<BTreeMap<String, UpdateRun>>,
}

impl UpdatesState {
    /// Takes the run lock without waiting and marks a run as in progress until the guard is
    /// dropped (also when the run panics).
    pub fn try_start(&self) -> Result<RunGuard, AlreadyRunning> {
        let lock = self
            .running
            .clone()
            .try_lock_owned()
            .map_err(|_| AlreadyRunning)?;
        Ok(RunGuard::new(lock, self.active.clone()))
    }

    /// Whether a run is in progress (a flag; the lock is not touched).
    pub fn is_running(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }
}

/// `run_all` was called while another run is in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlreadyRunning;

impl std::fmt::Display for AlreadyRunning {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("already running")
    }
}

impl std::error::Error for AlreadyRunning {}
