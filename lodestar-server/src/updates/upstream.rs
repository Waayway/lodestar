//! Latest upstream release of each tracked software.

use std::time::Duration;

use anyhow::Context;
use async_trait::async_trait;
use serde::Deserialize;

use super::version::tag_to_version;
use crate::clock;
use crate::config::Upstream;
use crate::state::AppState;

const TIMEOUT: Duration = Duration::from_secs(20);

/// What is known about a software's latest upstream release.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LatestVersion {
    pub version: Option<String>,
    /// Unix seconds of the last successful check.
    pub checked_at: Option<i64>,
    /// Why the last check failed; the last known `version` stays.
    pub error: Option<String>,
    /// Failed checks since start ("no stable release" included); a counter, never reset.
    pub errors: u64,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// The newest release in a GitHub releases list that is neither draft nor prerelease.
pub fn latest_release(json: &str) -> anyhow::Result<Option<String>> {
    let releases: Vec<Release> =
        serde_json::from_str(json).context("parsing the GitHub releases list")?;
    Ok(releases
        .iter()
        .find(|release| !release.draft && !release.prerelease)
        .map(|release| tag_to_version(&release.tag_name)))
}

/// Fetches the raw releases list of an upstream repository.
#[async_trait]
pub trait UpstreamProvider: Send + Sync {
    async fn releases(&self, repo: &str) -> anyhow::Result<String>;
}

pub struct GithubProvider {
    http: reqwest::Client,
}

impl GithubProvider {
    pub fn new() -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .build()
            .context("building HTTP client")?;
        Ok(Self { http })
    }
}

#[async_trait]
impl UpstreamProvider for GithubProvider {
    async fn releases(&self, repo: &str) -> anyhow::Result<String> {
        let url = format!("https://api.github.com/repos/{repo}/releases?per_page=20");
        let response = self
            .http
            .get(&url)
            .header("User-Agent", "lodestar")
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .with_context(|| format!("GET {url}"))?
            .error_for_status()
            .with_context(|| format!("GET {url}"))?;
        response
            .text()
            .await
            .with_context(|| format!("reading {url}"))
    }
}

/// Checks every tracked software once. A failure (or no stable release) keeps the last known
/// version and its time, records the error and counts it.
pub async fn check_once(state: &AppState) {
    for (name, software) in &state.config.updates.software {
        let Upstream::Github(repo) = &software.upstream;
        let result = match state.upstream.releases(repo).await {
            Ok(json) => latest_release(&json).and_then(|version| {
                version.ok_or_else(|| anyhow::anyhow!("{repo} has no stable release"))
            }),
            Err(error) => Err(error),
        };
        let mut latest = state.latest.lock().unwrap();
        let entry = latest.entry(name.clone()).or_default();
        match result {
            Ok(version) => {
                entry.version = Some(version);
                entry.checked_at = Some(clock::now());
                entry.error = None;
            }
            Err(error) => {
                eprintln!("lodestar-server: upstream check for {name}: {error:#}");
                entry.error = Some(format!("{error:#}"));
                entry.errors += 1;
            }
        }
    }
}
