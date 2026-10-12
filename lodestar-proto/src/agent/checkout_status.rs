use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct CheckoutStatus {
    pub branch: Option<String>,
    pub head: Option<String>,
    pub dirty: bool,
    pub untracked: u32,
    pub ahead: Option<u32>,
    #[ts(type = "number | null")]
    pub last_fetch_at: Option<i64>,
    /// Path of the checkout on the host (absent from older agents).
    #[serde(default)]
    pub flake_path: Option<String>,
}
