//! The result of one updater run for one flake input, and its audit-event form.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// One tracked software that got newer: lowest old -> highest new version across hosts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VersionChange {
    pub software: String,
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum UpdateOutcome {
    /// The bumped lock was pushed to the updates channel.
    Pushed {
        commit: String,
        changes: Vec<VersionChange>,
    },
    /// The lock did not change, or no tracked software got newer (or one would get older).
    NoChange,
    /// Evaluating a host's versions failed (an empty `host`: the input itself could not be
    /// updated).
    EvalFailed { host: String, log: String },
    /// Building (Linux) or evaluating the system (darwin) of a changed host failed.
    BuildFailed { host: String, log: String },
    /// The push (or preparing the worktree) failed; e.g. the channel moved.
    PushFailed { reason: String },
}

impl UpdateOutcome {
    /// The result name used in audit events and metrics.
    pub fn result(&self) -> &'static str {
        match self {
            UpdateOutcome::Pushed { .. } => "pushed",
            UpdateOutcome::NoChange => "no-change",
            UpdateOutcome::EvalFailed { .. } => "eval-failed",
            UpdateOutcome::BuildFailed { .. } => "build-failed",
            UpdateOutcome::PushFailed { .. } => "push-failed",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct UpdateRun {
    pub input: String,
    /// Unix seconds.
    pub finished_at: i64,
    pub outcome: UpdateOutcome,
}

impl UpdateRun {
    /// Audit details: `{"input", "result", "commit"?, "changes"?, "host"?, "log"?}`. A
    /// push failure's reason is its `log`; `note` (e.g. "downgrade") is added by the caller.
    pub fn details(&self) -> Value {
        let mut details = json!({ "input": self.input, "result": self.outcome.result() });
        let fields = details.as_object_mut().expect("object literal");
        match &self.outcome {
            UpdateOutcome::Pushed { commit, changes } => {
                fields.insert("commit".into(), json!(commit));
                fields.insert("changes".into(), json!(changes));
            }
            UpdateOutcome::NoChange => {}
            UpdateOutcome::EvalFailed { host, log } | UpdateOutcome::BuildFailed { host, log } => {
                if !host.is_empty() {
                    fields.insert("host".into(), json!(host));
                }
                fields.insert("log".into(), json!(log));
            }
            UpdateOutcome::PushFailed { reason } => {
                fields.insert("log".into(), json!(reason));
            }
        }
        details
    }

    /// The run recorded in an audit event's `details` at `time`; `None` when they are not an
    /// update run.
    pub fn from_details(time: i64, details: &Value) -> Option<UpdateRun> {
        let text = |key: &str| details.get(key).and_then(Value::as_str).map(str::to_string);
        let input = text("input")?;
        let log = text("log").unwrap_or_default();
        let outcome = match text("result")?.as_str() {
            "pushed" => UpdateOutcome::Pushed {
                commit: text("commit")?,
                changes: serde_json::from_value(details.get("changes")?.clone()).ok()?,
            },
            "no-change" => UpdateOutcome::NoChange,
            "eval-failed" => UpdateOutcome::EvalFailed {
                host: text("host").unwrap_or_default(),
                log,
            },
            "build-failed" => UpdateOutcome::BuildFailed {
                host: text("host").unwrap_or_default(),
                log,
            },
            "push-failed" => UpdateOutcome::PushFailed { reason: log },
            _ => return None,
        };
        Some(UpdateRun {
            input,
            finished_at: time,
            outcome,
        })
    }
}
