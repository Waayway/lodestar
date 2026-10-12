use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct ChannelView {
    pub name: String,
    pub head: Option<String>,
    pub head_subject: Option<String>,
}
