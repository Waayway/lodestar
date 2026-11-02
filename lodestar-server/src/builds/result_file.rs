//! `state_dir/builds/<commit>.json`: what the builder made of each host's system at a commit.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::ManagedHost;

/// The build results of one commit, by host name. An entry is final until `retry` removes it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BuildResult {
    pub commit: String,
    pub hosts: BTreeMap<String, HostResult>,
}

/// One host's build at a commit.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct HostResult {
    pub ok: bool,
    /// The system toplevel's store path, when it was built.
    pub path: Option<String>,
    /// Why it failed.
    pub log_tail: Option<String>,
    /// When the result was recorded (unix seconds).
    pub built_at: i64,
}

/// Whether the gate builds `host` (only Linux hosts; darwin is never built).
pub(crate) fn is_built_here(host: &ManagedHost) -> bool {
    host.os != "darwin"
}

impl BuildResult {
    /// A result with no hosts yet.
    pub fn empty(commit: &str) -> Self {
        Self {
            commit: commit.to_string(),
            hosts: BTreeMap::new(),
        }
    }

    /// The Linux hosts among `hosts` whose build failed, by name.
    pub fn failed_hosts(&self, hosts: &[ManagedHost]) -> Vec<String> {
        hosts
            .iter()
            .filter(|host| is_built_here(host))
            .filter(|host| self.hosts.get(&host.name).is_some_and(|result| !result.ok))
            .map(|host| host.name.clone())
            .collect()
    }

    /// The Linux hosts among `hosts` that have no result yet.
    pub fn missing(&self, hosts: &[ManagedHost]) -> Vec<ManagedHost> {
        hosts
            .iter()
            .filter(|host| is_built_here(host) && !self.hosts.contains_key(&host.name))
            .cloned()
            .collect()
    }
}
