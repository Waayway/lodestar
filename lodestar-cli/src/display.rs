//! Short text forms of commits and apply records.

use lodestar_proto::{ApplyRecord, ApplyState};

/// The first 8 characters of a commit.
pub(crate) fn short(commit: &str) -> &str {
    &commit[..commit.len().min(8)]
}

fn state_str(state: &ApplyState) -> &'static str {
    match state {
        ApplyState::Idle => "idle",
        ApplyState::Running => "running",
        ApplyState::Ok => "ok",
        ApplyState::Skipped => "skipped",
        ApplyState::Failed => "failed",
    }
}

pub(crate) fn is_terminal(state: &ApplyState) -> bool {
    matches!(
        state,
        ApplyState::Ok | ApplyState::Skipped | ApplyState::Failed
    )
}

pub(crate) fn describe(apply: &ApplyRecord) -> String {
    let mut text = state_str(&apply.state).to_string();
    if let Some(reason) = &apply.skip_reason {
        text.push_str(&format!(
            " ({})",
            serde_json::to_string(reason)
                .unwrap_or_default()
                .trim_matches('"')
        ));
    }
    if let Some(code) = apply.exit_code.filter(|code| *code != 0) {
        text.push_str(&format!(" (exit {code})"));
    }
    if let Some(message) = &apply.message {
        text.push_str(&format!(": {message}"));
    }
    text
}
