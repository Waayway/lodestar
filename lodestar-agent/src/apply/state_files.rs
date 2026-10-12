//! Paths and small helpers for the files under `<state_dir>`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::AgentConfig;

/// Records a successful fetch in `<state_dir>/last_fetch` (unix seconds).
pub fn record_fetch(config: &AgentConfig) {
    let _ = fs::create_dir_all(&config.state_dir);
    let _ = fs::write(config.state_dir.join("last_fetch"), now().to_string());
}

pub fn applies_dir(config: &AgentConfig) -> PathBuf {
    config.state_dir.join("applies")
}

/// Apply ids become file names, so anything that could escape the directory
/// is refused.
pub fn is_valid_apply_id(id: &str) -> bool {
    id.bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && id.len() <= 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// `applies/<id>.<extension>`.
pub(super) fn apply_file(config: &AgentConfig, id: &str, extension: &str) -> PathBuf {
    applies_dir(config).join(format!("{id}.{extension}"))
}

pub(super) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64)
}

/// Writes `path` atomically: a temporary sibling, then a rename.
pub(super) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)
}
