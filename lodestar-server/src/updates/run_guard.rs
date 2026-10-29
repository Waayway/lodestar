//! Proof that an updater run is in progress.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Holds the run lock and keeps the `running` flag set; dropping it (normally or while a
/// panic unwinds) clears the flag and releases the lock.
pub struct RunGuard {
    _lock: tokio::sync::OwnedMutexGuard<()>,
    active: Arc<AtomicBool>,
}

impl RunGuard {
    /// Sets the flag; call only after the lock was acquired.
    pub(super) fn new(lock: tokio::sync::OwnedMutexGuard<()>, active: Arc<AtomicBool>) -> RunGuard {
        active.store(true, Ordering::SeqCst);
        RunGuard {
            _lock: lock,
            active,
        }
    }
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        self.active.store(false, Ordering::SeqCst);
    }
}
