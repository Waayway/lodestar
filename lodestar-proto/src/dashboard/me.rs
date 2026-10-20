use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::Role;

/// Why the viewer may not act.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ActDenied {
    Anonymous,
    NotAdmin,
    NotTailnet,
}

/// Who the server thinks the viewer is (`/api/me`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct Me {
    pub viewer: Option<String>,
    pub role: Role,
    pub session: bool,
    pub can_act: bool,
    pub reason: Option<ActDenied>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn me_json() {
        let me = Me {
            viewer: Some("bob".into()),
            role: Role::Admin,
            session: true,
            can_act: false,
            reason: Some(ActDenied::NotTailnet),
        };
        assert_eq!(
            serde_json::to_string(&me).unwrap(),
            r#"{"viewer":"bob","role":"admin","session":true,"can_act":false,"reason":"not_tailnet"}"#
        );
    }
}
