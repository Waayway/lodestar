//! Reads the tracked software versions the system configuration writes.

use std::collections::BTreeMap;
use std::path::Path;

/// The software name to version map in `path`. A missing or unreadable file, or content that
/// is not a JSON object of strings, gives an empty map.
pub fn read_versions(path: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}
