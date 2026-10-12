use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{ApplyRecord, CheckoutStatus};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct AgentStatus {
    pub host: String,
    pub os: String,
    pub agent_version: String,
    pub running: String,
    pub checkout: CheckoutStatus,
    pub apply: Option<ApplyRecord>,
}
