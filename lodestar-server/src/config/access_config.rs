use serde::{Deserialize, Serialize};

/// Who may use the dashboard.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AccessConfig {
    Tailscale {
        #[serde(default)]
        allowed_users: Vec<String>,
        #[serde(default)]
        allowed_tags: Vec<String>,
    },
    Disabled,
}
