//! The apply lock: `<state_dir>/apply.lock`, held by the running wrapper.

use std::fs::{self, File, OpenOptions};

use crate::config::AgentConfig;

/// Exclusive, non-blocking lock on `<state_dir>/apply.lock`; held (by the
/// returned file) for as long as an apply runs. None if another holds it.
pub fn try_lock_apply(config: &AgentConfig) -> Option<File> {
    use std::os::fd::AsRawFd;
    fs::create_dir_all(&config.state_dir).ok()?;
    let lock_file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(config.state_dir.join("apply.lock"))
        .ok()?;
    // SAFETY: flock on a descriptor we own.
    (unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0)
        .then_some(lock_file)
}

/// `try_lock_apply`, retried for up to a second: the daemon briefly takes the lock to check
/// whether a `Running` record's wrapper is still alive (`read_record`).
pub(super) fn lock_apply_briefly(config: &AgentConfig) -> Option<File> {
    for _ in 0..20 {
        if let Some(lock_file) = try_lock_apply(config) {
            return Some(lock_file);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    try_lock_apply(config)
}
