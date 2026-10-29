use std::collections::BTreeMap;

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
    /// Tracked software versions (name to version); absent from old agents.
    #[serde(default)]
    pub versions: BTreeMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_without_versions_has_an_empty_map() {
        let json = r#"{"host":"h","os":"linux","agent_version":"0.1.0","running":"abc",
            "checkout":{"branch":null,"head":null,"dirty":false,"untracked":0,"ahead":null,
            "last_fetch_at":null,"flake_path":null},"apply":null}"#;
        let status: AgentStatus = serde_json::from_str(json).unwrap();
        assert!(status.versions.is_empty());
    }
}
