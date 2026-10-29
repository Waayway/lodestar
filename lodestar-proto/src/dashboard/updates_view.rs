use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// `GET /api/updates`: tracked software, the flake inputs the updater bumps, and whether a
/// run is in progress.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct UpdatesView {
    pub software: Vec<SoftwareView>,
    pub inputs: Vec<InputView>,
    pub running: bool,
}

/// One tracked software and its latest upstream release.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct SoftwareView {
    pub name: String,
    pub input: String,
    pub latest: Option<String>,
    #[ts(type = "number | null")]
    pub checked_at: Option<i64>,
    pub error: Option<String>,
}

/// A flake input the updater bumps and its last run.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct InputView {
    pub name: String,
    pub last: Option<UpdateRunView>,
}

/// The result of one run for one input.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct UpdateRunView {
    #[ts(type = "number")]
    pub finished_at: i64,
    /// `pushed`, `no-change`, `eval-failed`, `build-failed` or `push-failed`.
    pub result: String,
    pub commit: Option<String>,
    pub changes: Vec<VersionChangeView>,
    pub host: Option<String>,
}

/// One software that got newer in a pushed run.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct VersionChangeView {
    pub software: String,
    pub from: String,
    pub to: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updates_view_json() {
        let view = UpdatesView {
            software: vec![SoftwareView {
                name: "tailscale".into(),
                input: "nixpkgs".into(),
                latest: Some("1.98.10".into()),
                checked_at: Some(5),
                error: None,
            }],
            inputs: vec![InputView {
                name: "nixpkgs".into(),
                last: None,
            }],
            running: false,
        };
        assert_eq!(
            serde_json::to_string(&view).unwrap(),
            r#"{"software":[{"name":"tailscale","input":"nixpkgs","latest":"1.98.10","checked_at":5,"error":null}],"inputs":[{"name":"nixpkgs","last":null}],"running":false}"#
        );
    }
}
