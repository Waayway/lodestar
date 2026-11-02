//! A builder for the build gate's callers: counts calls, records which commit and hosts each
//! build had and every publish, fails chosen hosts (or all, as unreachable), fails publishes
//! and can block until released.

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
                    HostBuild {
                        ok: true,
                        path: Some(format!(
                            "/nix/store/0123456789abcdfghijklmnpqrsvwxyz-nixos-system-{}",
                            host.name.to_lowercase()
                        )),
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
        match self.publish_error.lock().unwrap().clone() {
            Some(message) => anyhow::bail!("{message}"),
            None => Ok(()),
        }
    }
}
