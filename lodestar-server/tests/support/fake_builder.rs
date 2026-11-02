//! A builder for the build gate's callers: counts calls, records which commit and hosts each
//! build had and every publish, fails chosen hosts (or all, as unreachable), fails publishes
//! and can block until released. It can also keep a store: every path it builds is in it,
//! `forget` drops one (a GC on the builder) and a publish of a path not in it fails.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use lodestar_server::builds::{Builder, HostBuild};
use lodestar_server::config::ManagedHost;
use tokio::sync::{Notify, Semaphore};

use super::git;

#[derive(Default)]
pub struct FakeBuilder {
    /// Every `build` call: (commit checked out, host names).
    pub calls: Mutex<Vec<(String, Vec<String>)>>,
    /// These hosts fail.
    pub fail: Mutex<HashSet<String>>,
    /// When set, `build` waits for a permit (tests add permits to release it).
    pub hold: Option<Arc<Semaphore>>,
    /// Signalled when `build` is entered.
    pub entered: Notify,
    /// Every host fails as "builder unreachable" (the probe failed).
    pub unreachable: AtomicBool,
    /// Every publish fails with this message.
    pub publish_error: Mutex<Option<String>>,
    /// Every publish: (commit, paths).
    pub published: Mutex<Vec<(String, Vec<String>)>>,
    /// When set, a publish fails for a path that is not in `store`.
    pub strict_store: AtomicBool,
    /// The paths `build` realised and the builder has not collected since.
    pub store: Mutex<HashSet<String>>,
}

/// The store path the fake builds for `host`.
pub fn path_of(host: &str) -> String {
    format!(
        "/nix/store/0123456789abcdfghijklmnpqrsvwxyz-nixos-system-{}",
        host.to_lowercase()
    )
}

impl FakeBuilder {
    pub fn failing(hosts: &[&str]) -> Self {
        let builder = Self::default();
        builder
            .fail
            .lock()
            .unwrap()
            .extend(hosts.iter().map(|host| host.to_string()));
        builder
    }

    pub fn held(hold: Arc<Semaphore>) -> Self {
        Self {
            hold: Some(hold),
            ..Self::default()
        }
    }

    pub fn calls(&self) -> Vec<(String, Vec<String>)> {
        self.calls.lock().unwrap().clone()
    }

    pub fn published(&self) -> Vec<(String, Vec<String>)> {
        self.published.lock().unwrap().clone()
    }

    pub fn set_unreachable(&self, unreachable: bool) {
        self.unreachable.store(unreachable, Ordering::SeqCst);
    }

    /// Publishes fail for paths the store does not have (as `cache-publish` does for a path
    /// that is neither on the builder nor in the cache).
    pub fn keep_store(&self) {
        self.strict_store.store(true, Ordering::SeqCst);
    }

    /// The builder's GC collected `host`'s path.
    pub fn forget(&self, host: &str) {
        self.store.lock().unwrap().remove(&path_of(host));
    }
}

#[async_trait]
impl Builder for FakeBuilder {
    async fn build(&self, dir: &Path, hosts: &[ManagedHost]) -> BTreeMap<String, HostBuild> {
        let commit = git(dir, &["rev-parse", "HEAD"]);
        self.calls
            .lock()
            .unwrap()
            .push((commit, hosts.iter().map(|host| host.name.clone()).collect()));
        self.entered.notify_one();
        if let Some(hold) = &self.hold {
            hold.acquire().await.unwrap().forget();
        }
        let fail = self.fail.lock().unwrap().clone();
        let unreachable = self.unreachable.load(Ordering::SeqCst);
        hosts
            .iter()
            .map(|host| {
                let build = if unreachable {
                    HostBuild {
                        ok: false,
                        path: None,
                        log_tail: Some(
                            "builder unreachable: error: cannot connect to 'alice@example.com'"
                                .into(),
                        ),
                    }
                } else if fail.contains(&host.name) {
                    HostBuild {
                        ok: false,
                        path: None,
                        log_tail: Some("error: builder said no".into()),
                    }
                } else {
                    self.store.lock().unwrap().insert(path_of(&host.name));
                    HostBuild {
                        ok: true,
                        path: Some(path_of(&host.name)),
                        log_tail: None,
                    }
                };
                (host.name.clone(), build)
            })
            .collect()
    }

    async fn publish(
        &self,
        commit: &str,
        paths: &[String],
        _channels: &serde_json::Value,
    ) -> anyhow::Result<()> {
        self.published
            .lock()
            .unwrap()
            .push((commit.to_string(), paths.to_vec()));
        if let Some(message) = self.publish_error.lock().unwrap().clone() {
            anyhow::bail!("{message}");
        }
        if self.strict_store.load(Ordering::SeqCst) {
            let store = self.store.lock().unwrap();
            if let Some(path) = paths.iter().find(|path| !store.contains(*path)) {
                anyhow::bail!("path '{path}' is not valid");
            }
        }
        Ok(())
    }
}
