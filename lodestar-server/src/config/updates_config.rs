use std::collections::BTreeMap;
use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

use super::ServerConfig;
use crate::mirror::valid_channel;

/// The software updater: which flake inputs it bumps and which versions it tracks.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct UpdatesConfig {
    pub channel: String,
    pub interval_secs: u64,
    pub upstream_interval_secs: u64,
    /// Software name -> flake input and upstream; empty turns the updater off.
    pub software: BTreeMap<String, SoftwareConfig>,
}

impl Default for UpdatesConfig {
    fn default() -> Self {
        Self {
            channel: "unstable".into(),
            interval_secs: 86400,
            upstream_interval_secs: 21600,
            software: BTreeMap::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SoftwareConfig {
    pub input: String,
    pub upstream: Upstream,
}

/// Where the latest release of a piece of software comes from.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Upstream {
    /// `owner/repo` on GitHub.
    Github(String),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MetricsConfig {
    pub listen: SocketAddr,
}

impl UpdatesConfig {
    /// Refuses names that must never reach a git or nix argument unchecked.
    pub fn check(&self) -> anyhow::Result<()> {
        if !valid_channel(&self.channel) {
            anyhow::bail!(
                "updates.channel {:?} is not a valid branch name",
                self.channel
            );
        }
        if self.interval_secs == 0 {
            anyhow::bail!("updates.interval_secs must be greater than 0");
        }
        if self.upstream_interval_secs == 0 {
            anyhow::bail!("updates.upstream_interval_secs must be greater than 0");
        }
        for (name, software) in &self.software {
            if name.is_empty()
                || !name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-'))
            {
                anyhow::bail!("updates.software name {name:?} must match [a-z0-9_-]+");
            }
            let input = &software.input;
            if input.is_empty()
                || !input
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
            {
                anyhow::bail!("updates.software.{name}.input {input:?} must match [A-Za-z0-9_-]+");
            }
            let Upstream::Github(repo) = &software.upstream;
            if !valid_github_repo(repo) {
                anyhow::bail!(
                    "updates.software.{name}.upstream.github {repo:?} must be owner/repo ([A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)"
                );
            }
        }
        Ok(())
    }

    /// Flake input -> the software names that track it.
    pub fn inputs(&self) -> BTreeMap<String, Vec<String>> {
        let mut inputs: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (name, software) in &self.software {
            inputs
                .entry(software.input.clone())
                .or_default()
                .push(name.clone());
        }
        inputs
    }
}

fn valid_github_repo(repo: &str) -> bool {
    let part = |text: &str| {
        !text.is_empty()
            && text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
    };
    matches!(repo.split_once('/'), Some((owner, name)) if part(owner) && part(name))
}

impl ServerConfig {
    /// Validates `updates`, and that the updater never targets the promote target branch.
    pub fn check_updates(&self) -> anyhow::Result<()> {
        self.updates.check()?;
        let protected = self.promote.as_ref().map_or("stable", |p| p.to.as_str());
        if self.updates.channel == protected {
            anyhow::bail!(
                "updates.channel {:?} is the promote target; the updater must not push there",
                self.updates.channel
            );
        }
        Ok(())
    }
}
