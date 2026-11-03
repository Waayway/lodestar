use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// `GET /api/builds/recent`: whether the build gate is on, and the newest builds on the builder.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct BuildsView {
    /// Whether the server builds every Linux host before it applies (`builds.enable`).
    pub enabled: bool,
    /// Newest first: commits building now, then recorded results by when they were written.
    pub builds: Vec<BuildView>,
}

/// What the builder made of one commit.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct BuildView {
    pub commit: String,
    /// The commit's subject line, when the mirror has the commit.
    pub subject: Option<String>,
    /// The channels building this commit right now (sorted).
    pub building: Vec<String>,
    /// The channels whose head this commit is (sorted).
    pub head_of: Vec<String>,
    /// The channels whose cache keeps this commit as current or previous good build (sorted).
    pub cached_for: Vec<String>,
    /// One entry per host with a result, by name.
    pub hosts: Vec<HostBuildView>,
    /// When the newest host result was recorded (unix seconds); none without results.
    #[ts(type = "number | null")]
    pub finished_at: Option<i64>,
}

/// One host's build at a commit.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct HostBuildView {
    pub name: String,
    pub ok: bool,
    /// The system toplevel's store path, when it was built.
    pub path: Option<String>,
    /// The end of the build's output (with the first errors) when it failed.
    pub log: Option<String>,
    /// When the result was recorded (unix seconds).
    #[ts(type = "number")]
    pub built_at: i64,
}
