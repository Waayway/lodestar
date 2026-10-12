use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct AuditEvent {
    #[ts(type = "number")]
    pub id: i64,
    #[ts(type = "number")]
    pub time: i64,
    pub host: Option<String>,
    pub kind: String,
    pub actor: String,
    pub details: serde_json::Value,
}
