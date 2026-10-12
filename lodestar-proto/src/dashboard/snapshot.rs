use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{ChannelView, HostView, PromoteChannels};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct Snapshot {
    pub hosts: Vec<HostView>,
    pub channels: Vec<ChannelView>,
    pub promote_candidate: Option<String>,
    /// The promote pair (`from` -> `to`), or null when promote is disabled.
    #[serde(default)]
    pub promote: Option<PromoteChannels>,
    pub viewer: String,
    /// True only when the server runs in demo mode (simulated hosts).
    #[serde(default)]
    pub demo: bool,
}
