use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{ActivationEvent, ApplyRecord, CheckoutStatus};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum OnlineState {
    Off,
    AgentUnreachable,
    On,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct HostView {
    pub name: String,
    pub os: String,
    pub channel: String,
    pub online: OnlineState,
    #[ts(type = "number | null")]
    pub last_seen: Option<i64>,
    pub running: Option<String>,
    pub running_dirty: bool,
    pub behind: Option<u32>,
    pub checkout: Option<CheckoutStatus>,
    pub last_apply: Option<ApplyRecord>,
    pub desired_apply_id: Option<String>,
    pub last_activation: Option<ActivationEvent>,
}
