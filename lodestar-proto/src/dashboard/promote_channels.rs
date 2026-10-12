use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Promote fast-forwards channel `to` to a commit on channel `from`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct PromoteChannels {
    pub from: String,
    pub to: String,
}

impl Default for PromoteChannels {
    fn default() -> Self {
        PromoteChannels {
            from: "unstable".into(),
            to: "stable".into(),
        }
    }
}
