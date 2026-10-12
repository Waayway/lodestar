//! Builds the `GET /status` document.

use std::process::Command;

use lodestar_proto::{AgentStatus, CheckoutStatus};

use crate::apply::latest_record;
use crate::config::AgentConfig;
use crate::git::Git;

fn revision_argv(config: &AgentConfig) -> Option<Vec<String>> {
    if let Some(command) = &config.revision_command {
        return Some(command.clone());
    }
    let program = match config.os.as_str() {
        "linux" => "/run/current-system/sw/bin/nixos-version",
        "darwin" => "/run/current-system/sw/bin/darwin-version",
        _ => return None,
    };
    Some(vec![program.to_string()])
}

/// The running system's configuration revision, or "unknown".
pub fn running_revision(config: &AgentConfig) -> String {
    let Some(argv) = revision_argv(config) else {
        return "unknown".into();
    };
    let Some((program, args)) = argv.split_first() else {
        return "unknown".into();
    };
    Command::new(program)
        .args(args)
        .arg("--configuration-revision")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|revision| !revision.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

pub fn last_fetch_at(config: &AgentConfig) -> Option<i64> {
    std::fs::read_to_string(config.state_dir.join("last_fetch"))
        .ok()?
        .trim()
        .parse()
        .ok()
}

pub fn collect(config: &AgentConfig, git: &Git) -> AgentStatus {
    AgentStatus {
        host: config.host.clone(),
        os: config.os.clone(),
        agent_version: env!("CARGO_PKG_VERSION").to_string(),
        running: running_revision(config),
        checkout: CheckoutStatus {
            branch: git.current_branch(),
            head: git.head(),
            dirty: git.is_dirty(),
            untracked: git.untracked_count(),
            ahead: git.ahead_of(&format!("refs/remotes/origin/{}", config.channel)),
            last_fetch_at: last_fetch_at(config),
            flake_path: Some(config.flake_path.display().to_string()),
        },
        apply: latest_record(config),
    }
}
