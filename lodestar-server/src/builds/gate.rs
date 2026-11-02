//! The build gate: one build per commit, guarded by `state_dir/builds/<commit>.lock` and recorded
//! in `state_dir/builds/<commit>.json`.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{Context, bail};
use lodestar_proto::is_commit_hash;
use tokio::task::JoinHandle;

use super::builder::is_unreachable;
use super::channel_heads::ChannelHeads;
use super::result_file::{BuildResult, HostResult, is_built_here};
use crate::clock;
use crate::config::ManagedHost;
use crate::mirror::Mirror;
use crate::state::AppState;

/// How long a result file is kept after it was last written, unless a channel still points at it.
const RETENTION_SECS: i64 = 30 * 24 * 60 * 60;
/// The gate's index of built channel heads (`{ channel: { current, previous } }`).
const INDEX_FILE: &str = "channels.json";
/// Serialises read-modify-write of the index (and the publish that sends it).
const INDEX_LOCK_FILE: &str = "channels.lock";

type ChannelIndex = BTreeMap<String, ChannelHeads>;

/// Builds each commit at most once: every path to building goes through `ensure_built` (or, for
/// the updater, `build_dir` + `record_built`). The lock is an exclusive `flock`, so a server that dies
/// mid-build releases it with the process; results are written atomically after build and
/// publish returned, so a crash leaves either no result or a complete one.
///
/// A channel's entry in the index (`channels.json`, what the cache keeps) only ever moves to the
/// channel's current mirror head, once every Linux host of the channel is built there; whenever it
/// moves, every host of the channel is realised on the builder again (its GC roots none of the
/// published paths) and every path of the channel at that commit is published again.
#[derive(Clone, Debug)]
pub struct BuildGate {
    /// `state_dir/builds`.
    dir: PathBuf,
}

impl BuildGate {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// The result of `commit` for the Linux hosts among `hosts`, building (and publishing) the
    /// ones that have none yet. Waits while another caller holds the commit's lock. Without
    /// Linux hosts it returns what is recorded and never waits.
    pub async fn ensure_built(
        &self,
        state: &AppState,
        commit: &str,
        hosts: &[ManagedHost],
    ) -> anyhow::Result<BuildResult> {
        if !is_commit_hash(commit) {
            bail!("invalid commit {commit:?}");
        }
        let hosts: Vec<ManagedHost> = hosts.iter().filter(|h| is_built_here(h)).cloned().collect();
        if hosts.is_empty() {
            return Ok(self
                .read(commit)
                .unwrap_or_else(|| BuildResult::empty(commit)));
        }
        // Its own task, so a caller that goes away (a closed request) cannot release the lock
        // while the build it started goes on.
        let gate = self.clone();
        let task_state = state.clone();
        let task_commit = commit.to_string();
        let result = join(
            commit,
            tokio::spawn(async move {
                gate.ensure_built_locked(&task_state, &task_commit, &hosts)
                    .await
            }),
        )
        .await??;
        if let Err(error) = self.cleanup(clock::now()).await {
            eprintln!("lodestar-server: cleaning up build results failed: {error:#}");
        }
        Ok(result)
    }

    async fn ensure_built_locked(
        &self,
        state: &AppState,
        commit: &str,
        hosts: &[ManagedHost],
    ) -> anyhow::Result<BuildResult> {
        let _lock = self.lock(self.lock_path(commit)).await?;
        let result = self
            .read(commit)
            .unwrap_or_else(|| BuildResult::empty(commit));
        let missing = result.missing(hosts);
        let channels = channels_of(hosts);
        // A corrupt index would fail the publish step; fail before spending a build on it.
        let index = self.read_index()?;
        // The hosts already recorded ok on a channel that may advance to this commit: the
        // advance publishes their paths, which the builder's GC may have collected since, so
        // they are realised again in the same call (instant while the builder still has them).
        let refresh = self
            .advancing_hosts(state, &result, &index, &channels)
            .await;
        let mut to_build: Vec<ManagedHost> = missing.iter().chain(&refresh).cloned().collect();
        to_build.sort_by(|left, right| left.name.cmp(&right.name));
        if to_build.is_empty() {
            return Ok(result);
        }

        let work = self.dir.join("work").join(commit);
        std::fs::create_dir_all(self.dir.join("work"))
            .with_context(|| format!("creating {}", self.dir.join("work").display()))?;
        state
            .mirror
            .worktree_at(commit, &work)
            .await
            .with_context(|| format!("checking out {commit} to build it"))?;
        // In a task of its own so the worktree is removed when the builder panics too.
        let builder = state.builder.clone();
        let build_dir = work.clone();
        let outcome = tokio::spawn(async move { builder.build(&build_dir, &to_build).await }).await;
        state.mirror.remove_worktree(&work).await;
        let builds = match outcome {
            Ok(builds) => builds,
            Err(error) if error.is_panic() => bail!("builder panicked while building {commit}"),
            Err(error) => bail!("build of {commit} was cancelled: {error}"),
        };
        // Not reaching the builder says nothing about the commit: those hosts stay missing (or
        // keep their recorded result), so the next call builds them again.
        let unreachable = builds
            .values()
            .find(|build| is_unreachable(build))
            .and_then(|build| build.log_tail.clone());
        let built_at = clock::now();
        let realised = |hosts: &[ManagedHost]| -> BTreeMap<String, HostResult> {
            hosts
                .iter()
                .filter(|host| !builds.get(&host.name).is_some_and(is_unreachable))
                .map(|host| {
                    let result = match builds.get(&host.name) {
                        Some(build) if build.ok && build.path.is_none() => {
                            failed("the builder reported no store path", built_at)
                        }
                        Some(build) => HostResult {
                            ok: build.ok,
                            path: build.path.clone(),
                            log_tail: build.log_tail.clone(),
                            built_at,
                        },
                        None => failed("the builder returned no result for this host", built_at),
                    };
                    (host.name.clone(), result)
                })
                .collect()
        };
        let (built, refreshed) = (realised(&missing), realised(&refresh));
        let result = self
            .publish_and_write(state, result, built, refreshed, &channels)
            .await?;
        match unreachable {
            Some(reason) => Err(anyhow::Error::msg(reason)),
            None => Ok(result),
        }
    }

    /// The Linux hosts recorded ok in `result` on each channel in `channels` that would advance
    /// to the commit once all its hosts are ok: the commit is the channel's mirror head and not
    /// its current build in `index`. Read without the index lock; `publish_and_write` checks
    /// again under it and only advances a channel all of whose hosts were realised in the call.
    async fn advancing_hosts(
        &self,
        state: &AppState,
        result: &BuildResult,
        index: &ChannelIndex,
        channels: &BTreeSet<String>,
    ) -> Vec<ManagedHost> {
        let mut hosts = Vec::new();
        for channel in channels {
            if index
                .get(channel)
                .is_some_and(|heads| heads.current == result.commit)
                || state.mirror.head(channel).await.as_deref() != Some(result.commit.as_str())
            {
                continue;
            }
            hosts.extend(
                state
                    .hosts
                    .iter()
                    .filter(|host| &host.channel == channel && is_built_here(host))
                    .filter(|host| {
                        result
                            .hosts
                            .get(&host.name)
                            .is_some_and(|host_result| host_result.ok)
                    })
                    .cloned(),
            );
        }
        hosts
    }

    /// Under the commit's lock: merges the hosts in `built` that `result` has no entry for yet
    /// (recorded results are final) into `result`, and of `refreshed` (recorded hosts realised
    /// again) the ones that now failed. Advances every channel in `channels` whose Linux hosts
    /// are all ok at the commit and were all realised in this call (in `built` or `refreshed`),
    /// when the commit is that channel's mirror head and not its current build yet, and
    /// publishes the new ok paths plus every ok path of the advanced channels (the cache may
    /// have pruned them). Writes the result and the index only after the publish succeeded; a
    /// failed publish records nothing and is an error.
    async fn publish_and_write(
        &self,
        state: &AppState,
        mut result: BuildResult,
        mut built: BTreeMap<String, HostResult>,
        refreshed: BTreeMap<String, HostResult>,
        channels: &BTreeSet<String>,
    ) -> anyhow::Result<BuildResult> {
        let commit = result.commit.clone();
        let realised: BTreeSet<String> = built.keys().chain(refreshed.keys()).cloned().collect();
        built.retain(|name, _| !result.hosts.contains_key(name));
        // A recorded host that fails now is a build failure like any other (final until a
        // retry); one that is ok again keeps its entry.
        built.extend(
            refreshed
                .into_iter()
                .filter(|(name, host_result)| !host_result.ok || !result.hosts.contains_key(name)),
        );
        let _index_lock = self.lock(self.dir.join(INDEX_LOCK_FILE)).await?;
        let mut index = self.read_index()?;
        result.hosts.extend(built.clone());

        let mut paths: BTreeSet<String> = built
            .values()
            .filter(|host_result| host_result.ok)
            .filter_map(|host_result| host_result.path.clone())
            .collect();
        let mut advanced = false;
        for channel in channels {
            let channel_hosts: Vec<&ManagedHost> = state
                .hosts
                .iter()
                .filter(|host| &host.channel == channel && is_built_here(host))
                .collect();
            let all_ok = !channel_hosts.is_empty()
                && channel_hosts.iter().all(|host| {
                    realised.contains(&host.name)
                        && result
                            .hosts
                            .get(&host.name)
                            .is_some_and(|host_result| host_result.ok)
                });
            if !all_ok
                || index
                    .get(channel)
                    .is_some_and(|heads| heads.current == commit)
            {
                continue;
            }
            if state.mirror.head(channel).await.as_deref() != Some(commit.as_str()) {
                continue;
            }
            let heads = ChannelHeads::advance(index.get(channel), &commit);
            index.insert(channel.clone(), heads);
            advanced = true;
            paths.extend(
                channel_hosts
                    .iter()
                    .filter_map(|host| result.hosts.get(&host.name))
                    .filter_map(|host_result| host_result.path.clone()),
            );
        }
        if !paths.is_empty() {
            let paths: Vec<String> = paths.into_iter().collect();
            let channels = serde_json::to_value(&index)?;
            if let Err(error) = state.builder.publish(&commit, &paths, &channels).await {
                bail!("publish failed: {error:#}");
            }
        }
        if !built.is_empty() {
            self.write_result(&result)?;
        }
        if advanced {
            write_atomically(
                &self.dir.join(INDEX_FILE),
                &serde_json::to_vec_pretty(&index)?,
            )
            .context("writing the build channel index after publishing")?;
        }
        Ok(result)
    }

    /// Builds `hosts` (Linux only) from the flake in `dir` without the lock and without
    /// publishing; the updater hands the results to `record_built` once it has a commit.
    pub async fn build_dir(
        &self,
        state: &AppState,
        dir: &Path,
        hosts: &[ManagedHost],
    ) -> BTreeMap<String, HostResult> {
        let hosts: Vec<ManagedHost> = hosts.iter().filter(|h| is_built_here(h)).cloned().collect();
        if hosts.is_empty() {
            return BTreeMap::new();
        }
        let builds = state.builder.build(dir, &hosts).await;
        let built_at = clock::now();
        hosts
            .iter()
            .map(|host| {
                let result = match builds.get(&host.name) {
                    Some(build) => HostResult {
                        ok: build.ok && build.path.is_some(),
                        path: build.path.clone(),
                        log_tail: build.log_tail.clone(),
                        built_at,
                    },
                    None => failed("the builder returned no result for this host", built_at),
                };
                (host.name.clone(), result)
            })
            .collect()
    }

    /// Records the updater's `build_dir` results for `commit` like `ensure_built` records its
    /// own: under the lock, hosts that have a result already keep it, the new ok paths are
    /// published and each channel of the recorded hosts advances when the commit is its head and
    /// all its Linux hosts are ok.
    ///
    /// The updater pushes the commit before calling this, so a prebuild after a mirror fetch in
    /// between may build the same commit once more; that is harmless (same derivations, already
    /// on the builder store) and its result wins here.
    pub async fn record_built(
        &self,
        state: &AppState,
        commit: &str,
        results: BTreeMap<String, HostResult>,
    ) -> anyhow::Result<BuildResult> {
        if !is_commit_hash(commit) {
            bail!("invalid commit {commit:?}");
        }
        let gate = self.clone();
        let task_state = state.clone();
        let task_commit = commit.to_string();
        join(
            commit,
            tokio::spawn(async move {
                let _lock = gate.lock(gate.lock_path(&task_commit)).await?;
                let result = gate
                    .read(&task_commit)
                    .unwrap_or_else(|| BuildResult::empty(&task_commit));
                // Every channel of a recorded host may advance (an update bumps hosts of
                // several channels at once).
                let hosts: Vec<ManagedHost> = task_state
                    .hosts
                    .iter()
                    .filter(|host| results.contains_key(&host.name) && is_built_here(host))
                    .cloned()
                    .collect();
                gate.publish_and_write(
                    &task_state,
                    result,
                    results,
                    BTreeMap::new(),
                    &channels_of(&hosts),
                )
                .await
            }),
        )
        .await?
    }

    /// Removes the failed hosts from `commit`'s result so the next `ensure_built` builds them
    /// again; returns their names.
    pub async fn retry(&self, commit: &str) -> anyhow::Result<Vec<String>> {
        if !is_commit_hash(commit) {
            bail!("invalid commit {commit:?}");
        }
        let _lock = self.lock(self.lock_path(commit)).await?;
        let Some(mut result) = self.read(commit) else {
            return Ok(Vec::new());
        };
        let failed: Vec<String> = result
            .hosts
            .iter()
            .filter(|(_, host_result)| !host_result.ok)
            .map(|(name, _)| name.clone())
            .collect();
        if !failed.is_empty() {
            result.hosts.retain(|_, host_result| host_result.ok);
            self.write_result(&result)?;
        }
        Ok(failed)
    }

    /// At server start, before any build: removes what a server that died mid-build left in
    /// `builds/work/` and prunes the mirror's registrations of those worktrees. Errors are
    /// logged; this is cleanup.
    pub async fn clear_stale_work(&self, mirror: &Mirror) {
        let work = self.dir.join("work");
        match std::fs::remove_dir_all(&work) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => eprintln!(
                "lodestar-server: removing stale build worktrees in {}: {error}",
                work.display()
            ),
        }
        mirror.prune_worktrees().await;
    }

    /// The recorded result of `commit`, if there is a readable one.
    pub fn read(&self, commit: &str) -> Option<BuildResult> {
        if !is_commit_hash(commit) {
            return None;
        }
        let text = std::fs::read_to_string(self.result_path(commit)).ok()?;
        let result: BuildResult = serde_json::from_str(&text).ok()?;
        (result.commit == commit).then_some(result)
    }

    /// Deletes result files (with their lock files) last written more than 30 days before `now`
    /// that no channel in the index has as current or previous commit. Commits whose lock is
    /// held are skipped. An unreadable index deletes nothing.
    pub async fn cleanup(&self, now: i64) -> anyhow::Result<()> {
        let gate = self.clone();
        tokio::task::spawn_blocking(move || gate.cleanup_blocking(now)).await?
    }

    fn cleanup_blocking(&self, now: i64) -> anyhow::Result<()> {
        let index = self.read_index()?;
        let keep: HashSet<&str> = index
            .values()
            .flat_map(|heads| std::iter::once(&heads.current).chain(heads.previous.as_ref()))
            .map(String::as_str)
            .collect();
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(error).with_context(|| format!("reading {}", self.dir.display()));
            }
        };
        let cutoff = now - RETENTION_SECS;
        for entry in entries {
            let name = entry?.file_name();
            let Some(commit) = name
                .to_str()
                .and_then(|name| name.strip_suffix(".json"))
                .filter(|commit| is_commit_hash(commit))
            else {
                continue;
            };
            if keep.contains(commit) || !self.written_before(commit, cutoff) {
                continue;
            }
            let lock_path = self.lock_path(commit);
            let lock = OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&lock_path)
                .with_context(|| format!("opening {}", lock_path.display()))?;
            match lock.try_lock() {
                Ok(()) => {}
                Err(TryLockError::WouldBlock) => continue,
                Err(TryLockError::Error(error)) => {
                    return Err(error).with_context(|| format!("locking {}", lock_path.display()));
                }
            }
            // Written again between the check and the lock: keep it.
            if !self.written_before(commit, cutoff) {
                continue;
            }
            let result_path = self.result_path(commit);
            std::fs::remove_file(&result_path)
                .with_context(|| format!("removing {}", result_path.display()))?;
            // Whoever waits on this lock notices it was removed and takes a new one.
            std::fs::remove_file(&lock_path)
                .with_context(|| format!("removing {}", lock_path.display()))?;
        }
        Ok(())
    }

    /// Whether `commit`'s result file was last modified before `cutoff` (unix seconds).
    fn written_before(&self, commit: &str, cutoff: i64) -> bool {
        std::fs::metadata(self.result_path(commit))
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .is_some_and(|age| (age.as_secs() as i64) < cutoff)
    }

    fn result_path(&self, commit: &str) -> PathBuf {
        self.dir.join(format!("{commit}.json"))
    }

    fn lock_path(&self, commit: &str) -> PathBuf {
        self.dir.join(format!("{commit}.lock"))
    }

    /// Takes the exclusive lock on `path`, waiting in a blocking task; dropping the file
    /// releases it.
    async fn lock(&self, path: PathBuf) -> anyhow::Result<File> {
        std::fs::create_dir_all(&self.dir)
            .with_context(|| format!("creating {}", self.dir.display()))?;
        tokio::task::spawn_blocking(move || lock_blocking(&path)).await?
    }

    /// The channel index; empty when there is none yet.
    fn read_index(&self) -> anyhow::Result<ChannelIndex> {
        let path = self.dir.join(INDEX_FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ChannelIndex::new()),
            Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
        }
    }

    fn write_result(&self, result: &BuildResult) -> anyhow::Result<()> {
        write_atomically(
            &self.result_path(&result.commit),
            &serde_json::to_vec_pretty(result)?,
        )
    }
}

/// Opens (creating) and locks `path`. When the file was removed or replaced while we waited
/// (cleanup deletes the lock files of old results), the lock is on a file nobody else sees, so
/// it starts over.
fn lock_blocking(path: &Path) -> anyhow::Result<File> {
    loop {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)
            .with_context(|| format!("opening {}", path.display()))?;
        file.lock()
            .with_context(|| format!("locking {}", path.display()))?;
        let held = file.metadata()?;
        match std::fs::metadata(path) {
            Ok(current) if current.dev() == held.dev() && current.ino() == held.ino() => {
                return Ok(file);
            }
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("reading {}", path.display()));
            }
        }
    }
}

/// Writes `contents` to `<path>.tmp`, syncs it and renames it over `path`.
fn write_atomically(path: &Path, contents: &[u8]) -> anyhow::Result<()> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    let mut file =
        File::create(&temporary).with_context(|| format!("creating {}", temporary.display()))?;
    file.write_all(contents)
        .and_then(|()| file.sync_all())
        .with_context(|| format!("writing {}", temporary.display()))?;
    std::fs::rename(&temporary, path)
        .with_context(|| format!("renaming {} to {}", temporary.display(), path.display()))?;
    if let Some(parent) = path.parent()
        && let Ok(directory) = File::open(parent)
    {
        let _ = directory.sync_all();
    }
    Ok(())
}

fn failed(reason: &str, built_at: i64) -> HostResult {
    HostResult {
        ok: false,
        path: None,
        log_tail: Some(reason.to_string()),
        built_at,
    }
}

/// The channels of the Linux hosts among `hosts`.
fn channels_of(hosts: &[ManagedHost]) -> BTreeSet<String> {
    hosts
        .iter()
        .filter(|host| is_built_here(host))
        .map(|host| host.channel.clone())
        .collect()
}

/// Awaits the locked work on `commit`; a panic there (its locks are released by then) is an
/// error.
async fn join<T>(commit: &str, task: JoinHandle<T>) -> anyhow::Result<T> {
    match task.await {
        Ok(value) => Ok(value),
        Err(error) if error.is_panic() => bail!("build of {commit} panicked"),
        Err(error) => bail!("build of {commit} was cancelled: {error}"),
    }
}
