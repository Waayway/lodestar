use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::ActivationEvent;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct ActivationsResponse {
    pub events: Vec<ActivationEvent>,
    #[ts(type = "number")]
    pub next: u64,
}
