use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::OnlineState;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ServiceState {
    Up,
    Down,
    Pending,
    Unchecked,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case", tag = "kind", content = "detail")]
#[ts(export)]
pub enum DownReason {
    HttpStatus(u16),
    Timeout,
    ConnectionRefused,
    Unreachable,
    Tls,
    Other(String),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct ServiceCheck {
    pub state: ServiceState,
    pub reason: Option<DownReason>,
    pub latency_ms: Option<u32>,
    #[ts(type = "number | null")]
    pub checked_at: Option<i64>,
    /// When the current state began.
    #[ts(type = "number | null")]
    pub since: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct ServiceView {
    pub id: String,
    pub title: String,
    pub url: String,
    pub group: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub host: String,
    pub host_online: Option<OnlineState>,
    pub check: ServiceCheck,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn down_reason_is_tagged() {
        let json = serde_json::to_string(&DownReason::HttpStatus(502)).unwrap();
        assert_eq!(json, r#"{"kind":"http_status","detail":502}"#);
        let json = serde_json::to_string(&DownReason::Timeout).unwrap();
        assert_eq!(json, r#"{"kind":"timeout"}"#);
    }
}
