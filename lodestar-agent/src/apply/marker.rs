//! `<state_dir>/apply-in-progress.json`, read by the activation audit hook.

use std::fs;
use std::path::PathBuf;

use super::lock::try_lock_apply;
use crate::config::AgentConfig;

pub(super) fn in_progress_marker(config: &AgentConfig) -> PathBuf {
    config.state_dir.join("apply-in-progress.json")
}

/// Removes a leftover `apply-in-progress.json` (a wrapper that crashed or a reboot mid-apply),
/// so the audit hook does not label later manual switches as that apply. Only when no wrapper
/// holds the apply lock. The daemon calls this at start.
pub fn clear_stale_marker(config: &AgentConfig) {
    let marker = in_progress_marker(config);
    if !marker.exists() {
        return;
    }
    // The lock binding lives until the end of the block.
    if let Some(_lock) = try_lock_apply(config)
        && fs::remove_file(&marker).is_ok()
    {
        eprintln!("lodestar-agent: removed stale apply-in-progress.json");
    }
}
