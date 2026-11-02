use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The build of a channel's newest commit on the builder.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct ChannelBuildView {
    /// The commit that is building or was built.
    pub commit: String,
    /// `building`, `ok` or `failed`.
    pub state: String,
    /// The hosts whose build failed (sorted); empty unless `state` is `failed`.
    pub failed_hosts: Vec<String>,
    /// When the newest host result was recorded (unix seconds); none while building.
    pub finished_at: Option<i64>,
}
