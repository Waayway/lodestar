//! The updater: bumps a flake input in a scratch worktree of the updates channel and pushes
//! the new `flake.lock` only when a tracked software got newer on some host and every
//! changed host still builds (design section 5, steps 1-7).

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::Path;

use super::nix_runner::log_tail;
use super::run_guard::RunGuard;
use super::update_run::{UpdateOutcome, UpdateRun, VersionChange};
use super::updates_state::AlreadyRunning;
use super::version::compare_versions;
use crate::builds::is_built_here;
use crate::clock;
use crate::config::ManagedHost;
use crate::db::{insert_audit, recent_audit_of_kind};
use crate::mirror::PushError;
use crate::state::AppState;

/// How many `update` audit events `restore_last` looks at.
const RESTORE_LIMIT: u64 = 1000;

type Versions = BTreeMap<String, String>;

/// `chore(<input>): bump for <software> <from> -> <to>[, …]`, in software-name order.
pub fn commit_message(input: &str, changes: &[VersionChange]) -> String {
    let mut changes: Vec<&VersionChange> = changes.iter().collect();
    changes.sort_by(|a, b| a.software.cmp(&b.software));
    let list: Vec<String> = changes
        .iter()
        .map(|change| format!("{} {} -> {}", change.software, change.from, change.to))
        .collect();
    format!("chore({input}): bump for {}", list.join(", "))
}

/// Runs every input that some tracked software uses, one after the other. Refuses (without
/// waiting) when another run is in progress.
pub async fn run_all(state: &AppState) -> Result<Vec<UpdateRun>, AlreadyRunning> {
    let running = state.updates.try_start()?;
    Ok(run_all_locked(state, running).await)
}

/// [`run_all`] for a caller that already started the run (the API does before answering, so
/// a status read right after sees it). The guard keeps the run marked as in progress until
/// every input is done, and is dropped on every exit, a panic included.
pub async fn run_all_locked(state: &AppState, running: RunGuard) -> Vec<UpdateRun> {
    let mut runs = Vec::new();
    for (input, software) in state.config.updates.inputs() {
        runs.push(run_input(state, &input, &software).await);
    }
    drop(running);
    runs
}

/// One run for `input`; only `software` counts as a change. The result is recorded as an
/// audit event and as the input's last run. The worktree is removed afterwards.
pub async fn run_input(state: &AppState, input: &str, software: &[String]) -> UpdateRun {
    let (outcome, extra) = if valid_input(input) {
        let dir = state.config.state_dir.join("updater").join(input);
        let result = attempt(state, input, software, &dir).await;
        state.mirror.remove_worktree(&dir).await;
        result
    } else {
        let log = format!("invalid input name {input:?}");
        (
            UpdateOutcome::EvalFailed {
                host: String::new(),
                log,
            },
            None,
        )
    };
    let run = UpdateRun {
        input: input.to_string(),
        finished_at: clock::now(),
        outcome,
    };
    let mut details = run.details();
    if let Some((key, value)) = extra {
        details[key] = value.into();
    }
    eprintln!("lodestar-server: update {input}: {}", run.outcome.result());
    if let Err(error) = insert_audit(
        &state.db,
        run.finished_at,
        None,
        "update",
        "lodestar",
        details,
    )
    .await
    {
        eprintln!("lodestar-server: recording update of {input}: {error:#}");
    }
    state
        .updates
        .last
        .lock()
        .unwrap()
        .insert(input.to_string(), run.clone());
    run
}

/// Restores the last run of every configured input from the newest `update` audit event.
pub async fn restore_last(state: &AppState) {
    let inputs = state.config.updates.inputs();
    let events = match recent_audit_of_kind(&state.db, "update", RESTORE_LIMIT).await {
        Ok(events) => events,
        Err(error) => {
            eprintln!("lodestar-server: restoring last update runs: {error:#}");
            return;
        }
    };
    let mut last = state.updates.last.lock().unwrap();
    // Newest first: the first event per input wins.
    for event in events {
        if let Some(run) = UpdateRun::from_details(event.time, &event.details)
            && inputs.contains_key(&run.input)
        {
            last.entry(run.input.clone()).or_insert(run);
        }
    }
}

/// Same rule as the config check, and never something nix could read as an option.
fn valid_input(input: &str) -> bool {
    !input.is_empty()
        && !input.starts_with('-')
        && input
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

/// Host names become part of a flake attribute path.
fn valid_host_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

/// Steps 1-6 in the worktree at `dir`; the second value is an extra audit detail (key, value).
async fn attempt(
    state: &AppState,
    input: &str,
    software: &[String],
    dir: &Path,
) -> (UpdateOutcome, Option<(&'static str, &'static str)>) {
    let channel = &state.config.updates.channel;
    // 1. Fresh worktree of the channel head (the mirror fetches first).
    if let Some(parent) = dir.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        let reason = format!("creating {}: {error}", parent.display());
        return (UpdateOutcome::PushFailed { reason }, None);
    }
    let base = match state.mirror.worktree(channel, dir).await {
        Ok(base) => base,
        Err(error) => {
            let reason = format!("preparing the worktree: {error:#}");
            return (UpdateOutcome::PushFailed { reason }, None);
        }
    };
    if let Some(host) = state.hosts.iter().find(|host| !valid_host_name(&host.name)) {
        let outcome = UpdateOutcome::EvalFailed {
            host: host.name.clone(),
            log: format!("invalid host name {:?}", host.name),
        };
        return (outcome, None);
    }
    // 2. Versions at the current lock.
    let before = match eval_all(state, dir, software).await {
        Ok(versions) => versions,
        Err(outcome) => return (outcome, None),
    };
    // 3. Bump the input.
    let lock = dir.join("flake.lock");
    let old_lock = std::fs::read(&lock).ok();
    if let Err(error) = state.nix.flake_update(dir, input).await {
        let outcome = UpdateOutcome::EvalFailed {
            host: String::new(),
            log: log_tail(&format!("{error:#}")),
        };
        return (outcome, None);
    }
    if !lock.is_file() {
        eprintln!("lodestar-server: update {input}: flake.lock is missing after the update");
        let outcome = UpdateOutcome::EvalFailed {
            host: String::new(),
            log: format!("flake.lock is missing after `nix flake update {input}`"),
        };
        return (outcome, None);
    }
    if std::fs::read(&lock).ok() == old_lock {
        return (UpdateOutcome::NoChange, None);
    }
    // 4. Versions at the new lock; push only if something got newer and nothing older.
    let after = match eval_all(state, dir, software).await {
        Ok(versions) => versions,
        Err(outcome) => return (outcome, None),
    };
    let comparison = compare(&state.hosts, &before, &after, software);
    if comparison.downgrade {
        return (UpdateOutcome::NoChange, Some(("note", "downgrade")));
    }
    if comparison.changes.is_empty() {
        return (UpdateOutcome::NoChange, None);
    }
    // 5. Build every host whose tracked versions changed. With the build gate the Linux hosts
    // build on the builder in one call (darwin hosts are only evaluated here); the results are
    // recorded for the pushed commit below.
    let gated = state.config.builds.enable;
    let mut built = BTreeMap::new();
    if gated {
        let linux: Vec<ManagedHost> = comparison
            .changed_hosts
            .iter()
            .filter(|host| is_built_here(host))
            .map(|host| (*host).clone())
            .collect();
        built = state.builds.build_dir(state, dir, &linux).await;
        // BTreeMap order: the first failed host by name.
        if let Some((host, result)) = built.iter().find(|(_, result)| !result.ok) {
            let log = result.log_tail.clone().unwrap_or_default();
            let outcome = UpdateOutcome::BuildFailed {
                host: host.clone(),
                log: log_tail(&log),
            };
            return (outcome, None);
        }
    }
    for host in &comparison.changed_hosts {
        if gated && is_built_here(host) {
            continue;
        }
        if let Err(log) = state.nix.build(dir, host).await {
            let outcome = UpdateOutcome::BuildFailed {
                host: host.name.clone(),
                log: log_tail(&log),
            };
            return (outcome, None);
        }
    }
    // 6. Commit flake.lock and push, fast-forward from `base` only.
    let message = commit_message(input, &comparison.changes);
    let outcome = match state.mirror.push_lock(dir, channel, &base, &message).await {
        Ok(commit) => UpdateOutcome::Pushed {
            commit,
            changes: comparison.changes,
        },
        Err(error @ PushError::ChannelMoved { .. }) => UpdateOutcome::PushFailed {
            reason: error.to_string(),
        },
        Err(error) => UpdateOutcome::PushFailed {
            reason: format!("push failed: {error}"),
        },
    };
    let mut extra = None;
    if gated
        && !built.is_empty()
        && let UpdateOutcome::Pushed { commit, .. } = &outcome
    {
        // The commit is on the remote already: a failed record keeps the outcome and shows in
        // the audit. A failed publish records nothing, so the next prebuild (or apply) of the
        // commit builds it again (already on the builder store) and publishes it.
        let names: Vec<String> = built.keys().cloned().collect();
        match state.builds.record_built(state, commit, built).await {
            Ok(result)
                if names
                    .iter()
                    .all(|name| result.hosts.get(name).is_some_and(|h| h.ok)) => {}
            Ok(_) => {
                eprintln!("lodestar-server: update {input}: publishing {commit} failed");
                extra = Some(("publish", "failed"));
            }
            Err(error) => {
                eprintln!("lodestar-server: update {input}: recording {commit}: {error:#}");
                extra = Some(("publish", "failed"));
            }
        }
    }
    (outcome, extra)
}

/// Tracked versions (only `software`) of every host; the first failing host stops it.
async fn eval_all(
    state: &AppState,
    dir: &Path,
    software: &[String],
) -> Result<BTreeMap<String, Versions>, UpdateOutcome> {
    let mut all = BTreeMap::new();
    for host in &state.hosts {
        match state.nix.eval_versions(dir, host).await {
            Ok(mut versions) => {
                versions.retain(|name, _| software.contains(name));
                all.insert(host.name.clone(), versions);
            }
            Err(error) => {
                return Err(UpdateOutcome::EvalFailed {
                    host: host.name.clone(),
                    log: log_tail(&format!("{error:#}")),
                });
            }
        }
    }
    Ok(all)
}

struct Comparison<'a> {
    /// Software that got newer on some host: lowest old -> highest new, by name.
    changes: Vec<VersionChange>,
    /// Hosts whose tracked versions differ at all, in hosts-file order.
    changed_hosts: Vec<&'a ManagedHost>,
    /// Some tracked software would get older on some host.
    downgrade: bool,
}

fn compare<'a>(
    hosts: &'a [ManagedHost],
    before: &BTreeMap<String, Versions>,
    after: &BTreeMap<String, Versions>,
    software: &[String],
) -> Comparison<'a> {
    let empty = Versions::new();
    let mut changes: BTreeMap<&str, VersionChange> = BTreeMap::new();
    let mut changed_hosts = Vec::new();
    let mut downgrade = false;
    for host in hosts {
        let old = before.get(&host.name).unwrap_or(&empty);
        let new = after.get(&host.name).unwrap_or(&empty);
        if old != new {
            changed_hosts.push(host);
        }
        for name in software {
            let (Some(from), Some(to)) = (old.get(name), new.get(name)) else {
                continue;
            };
            if from == to {
                continue;
            }
            match compare_versions(to, from) {
                Some(Ordering::Greater) => {
                    let change = changes.entry(name).or_insert_with(|| VersionChange {
                        software: name.clone(),
                        from: from.clone(),
                        to: to.clone(),
                    });
                    if compare_versions(from, &change.from) == Some(Ordering::Less) {
                        change.from = from.clone();
                    }
                    if compare_versions(to, &change.to) == Some(Ordering::Greater) {
                        change.to = to.clone();
                    }
                }
                Some(Ordering::Less) => downgrade = true,
                // Equal after normalisation, or incomparable: not a move forward.
                _ => {}
            }
        }
    }
    Comparison {
        changes: changes.into_values().collect(),
        changed_hosts,
        downgrade,
    }
}
