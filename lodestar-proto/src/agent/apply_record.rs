use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ApplyState {
    Idle,
    Running,
    Ok,
    Skipped,
    Failed,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SkipReason {
    FetchFailed,
    CommitNotFound,
    CommitNotOnChannel,
    WrongBranch,
    Dirty,
    NotFastForward,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct ApplyRecord {
    pub apply_id: String,
    pub commit: String,
    pub state: ApplyState,
    pub skip_reason: Option<SkipReason>,
    pub message: Option<String>,
    pub exit_code: Option<i32>,
    #[ts(type = "number | null")]
    pub started_at: Option<i64>,
    #[ts(type = "number | null")]
    pub finished_at: Option<i64>,
}
