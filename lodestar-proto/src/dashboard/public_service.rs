use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::ServiceState;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PublicServiceState {
    Up,
    Down,
    Unknown,
}

impl PublicServiceState {
    pub fn from_check(state: &ServiceState) -> PublicServiceState {
        match state {
            ServiceState::Up => PublicServiceState::Up,
            ServiceState::Down => PublicServiceState::Down,
            ServiceState::Pending | ServiceState::Unchecked => PublicServiceState::Unknown,
        }
    }
}

/// What an anonymous visitor may see of a service.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct PublicService {
    pub title: String,
    pub group: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub state: PublicServiceState,
    #[ts(type = "number | null")]
    pub since: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ServiceState;

    #[test]
    fn public_service_json() {
        let service = PublicService {
            title: "Grafana".into(),
            group: "Monitoring".into(),
            description: None,
            icon: None,
            state: PublicServiceState::Unknown,
            since: None,
        };
        assert_eq!(
            serde_json::to_string(&service).unwrap(),
            r#"{"title":"Grafana","group":"Monitoring","description":null,"icon":null,"state":"unknown","since":null}"#
        );
    }

    #[test]
    fn state_from_check() {
        assert_eq!(
            PublicServiceState::from_check(&ServiceState::Up),
            PublicServiceState::Up
        );
        assert_eq!(
            PublicServiceState::from_check(&ServiceState::Down),
            PublicServiceState::Down
        );
        assert_eq!(
            PublicServiceState::from_check(&ServiceState::Pending),
            PublicServiceState::Unknown
        );
        assert_eq!(
            PublicServiceState::from_check(&ServiceState::Unchecked),
            PublicServiceState::Unknown
        );
    }
}
