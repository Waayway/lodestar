//! Removes old applies.

use std::fs;

use lodestar_proto::ApplyState;

use super::record::records_newest_first;
use super::state_files::apply_file;
use crate::config::AgentConfig;

/// Deletes all but the newest `keep` applies (never one still running).
pub fn prune(config: &AgentConfig, keep: usize) {
    for record in records_newest_first(config).into_iter().skip(keep) {
        if record.state == ApplyState::Running {
            continue;
        }
        for extension in ["json", "log", "pid"] {
            let _ = fs::remove_file(apply_file(config, &record.apply_id, extension));
        }
    }
}
