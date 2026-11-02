use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::ChannelBuildView;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct ChannelView {
    pub name: String,
    pub head: Option<String>,
    pub head_subject: Option<String>,
    /// The head's build on the builder; none while builds are off or the head has no result yet.
    pub build: Option<ChannelBuildView>,
}
