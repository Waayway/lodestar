//! The updater's shared state: one run at a time, and the last run per input.

use std::collections::BTreeMap;

use super::update_run::UpdateRun;

#[derive(Default)]
pub struct UpdatesState {
    /// Held for a whole `run_all`; a second one fails with `AlreadyRunning`.
    pub running: tokio::sync::Mutex<()>,
    /// Last run per flake input (restored from the audit log at start).
    pub last: std::sync::Mutex<BTreeMap<String, UpdateRun>>,
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
