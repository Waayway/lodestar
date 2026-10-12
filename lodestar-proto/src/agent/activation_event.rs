use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct ActivationEvent {
    #[ts(type = "number")]
    pub line: u64,
    #[ts(type = "number")]
    pub time: i64,
    pub store_path: String,
    pub revision: String,
    #[ts(type = "number | null")]
    pub generation: Option<u64>,
    pub trigger: String,
    pub sudo_user: Option<String>,
}
