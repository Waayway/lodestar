//! Version comparison for tracked software.

use std::cmp::Ordering;

/// Parses a version into numeric parts: strips one leading `v`, cuts at the
/// first `-` or `+`, splits on dots. Any non-numeric part gives `None`.
pub fn parse_version(s: &str) -> Option<Vec<u64>> {
    let s = s.strip_prefix('v').unwrap_or(s);
    let core = s.split(['-', '+']).next().unwrap_or("");
    core.split('.').map(|p| p.parse::<u64>().ok()).collect()
}

/// Compares two versions numerically, a missing part counting as 0.
/// `None` when either side is not comparable.
pub fn compare_versions(a: &str, b: &str) -> Option<Ordering> {
    let (mut a, mut b) = (parse_version(a)?, parse_version(b)?);
    let len = a.len().max(b.len());
    a.resize(len, 0);
    b.resize(len, 0);
    Some(a.cmp(&b))
}

/// A release tag as a version: strips one leading `v`.
pub fn tag_to_version(tag: &str) -> String {
    tag.strip_prefix('v').unwrap_or(tag).to_string()
}
