//! The per-apply record, `applies/<id>.json`.

use std::fs;

use anyhow::Context;
use lodestar_proto::{ApplyRecord, ApplyState};

use super::lock::try_lock_apply;
use super::marker::in_progress_marker;
use super::state_files::{applies_dir, apply_file, is_valid_apply_id, now, write_atomic};
use crate::config::AgentConfig;

pub(super) fn write_record(config: &AgentConfig, record: &ApplyRecord) -> anyhow::Result<()> {
    write_atomic(
        &apply_file(config, &record.apply_id, "json"),
        &serde_json::to_vec(record)?,
    )
    .with_context(|| format!("writing record {}", record.apply_id))
}

/// True unless `pid` is known not to exist (`ESRCH`).
fn pid_alive(pid: i32) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    if unsafe { libc::kill(pid, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

/// The stored record. A `Running` record whose wrapper is gone is reported (and, when no
/// wrapper holds the apply lock, stored) as failed ("interrupted"). The wrapper is gone when
/// its pid is dead, or when the apply lock is free: a running wrapper always holds it, which
/// also catches a pid reused by an unrelated process.
pub fn read_record(config: &AgentConfig, id: &str) -> Option<ApplyRecord> {
    if !is_valid_apply_id(id) {
        return None;
    }
    let record = load_record(config, id)?;
    Some(settle_running(config, record))
}

fn load_record(config: &AgentConfig, id: &str) -> Option<ApplyRecord> {
    let raw = fs::read(apply_file(config, id, "json")).ok()?;
    serde_json::from_slice(&raw).ok()
}

/// Second half of `read_record`: `record` was read from disk (without the lock) and may be
/// stale. The wrapper can finish between that read and the checks here (write its final
/// record, remove the pid file and the marker, drop the lock), so a `Running` record is
/// re-read before it is declared interrupted, and only rewritten while it is still `Running`
/// on disk under the lock. Public for tests.
#[doc(hidden)]
pub fn settle_running(config: &AgentConfig, record: ApplyRecord) -> ApplyRecord {
    if record.state != ApplyState::Running {
        return record;
    }
    let id = record.apply_id.clone();
    let wrapper_alive = fs::read_to_string(apply_file(config, &id, "pid"))
        .ok()
        .and_then(|contents| contents.trim().parse::<i32>().ok())
        .is_some_and(pid_alive);
    // Held only for this check (and the clean-up below), never across a wait.
    let lock = try_lock_apply(config);
    if wrapper_alive && lock.is_none() {
        return record;
    }
    // The wrapper may have finished since `record` was read: what is on disk now wins. With
    // the lock held no wrapper can change it any more.
    let mut current = match load_record(config, &id) {
        Some(on_disk) if on_disk.state != ApplyState::Running => return on_disk,
        Some(on_disk) => on_disk,
        None => record,
    };
    current.state = ApplyState::Failed;
    current.message = Some("interrupted".into());
    if lock.is_some() {
        // No wrapper can be running: make the outcome permanent and drop the marker.
        if current.finished_at.is_none() {
            current.finished_at = Some(now());
        }
        if let Err(error) = write_record(config, &current) {
            eprintln!("lodestar-agent: {error:#}");
        }
        let _ = fs::remove_file(apply_file(config, &id, "pid"));
        let _ = fs::remove_file(in_progress_marker(config));
    }
    current
}

fn record_ids(config: &AgentConfig) -> Vec<String> {
    let Ok(entries) = fs::read_dir(applies_dir(config)) else {
        return vec![];
    };
    entries
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension()? != "json" {
                return None;
            }
            let id = path.file_stem()?.to_str()?.to_string();
            is_valid_apply_id(&id).then_some(id)
        })
        .collect()
}

/// Records, newest first (by `started_at`, then id).
pub(super) fn records_newest_first(config: &AgentConfig) -> Vec<ApplyRecord> {
    let mut records: Vec<ApplyRecord> = record_ids(config)
        .iter()
        .filter_map(|id| read_record(config, id))
        .collect();
    records.sort_by(|left, right| {
        (right.started_at, &right.apply_id).cmp(&(left.started_at, &left.apply_id))
    });
    records
}

pub fn latest_record(config: &AgentConfig) -> Option<ApplyRecord> {
    records_newest_first(config).into_iter().next()
}
