//! The apply wrapper body: fetch, checks, merge, build and switch.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::Context;
use lodestar_proto::{ApplyRecord, ApplyState, SkipReason, is_commit_hash};

use super::apply_log::ApplyLog;
use super::lock::lock_apply_briefly;
use super::marker::in_progress_marker;
use super::outcome::Outcome;
use super::prune::prune;
use super::record::write_record;
use super::state_files::{
    applies_dir, apply_file, is_valid_apply_id, now, record_fetch, write_atomic,
};
use crate::checks::check_apply;
use crate::config::AgentConfig;
use crate::git::Git;

const SYSTEM_PROFILE: &str = "/nix/var/nix/profiles/system";

/// Steps 5-6: build as the user, then switch as root (or the test command).
fn build_and_switch(
    config: &AgentConfig,
    git: &Git,
    commit: &str,
    log: &mut ApplyLog,
) -> Result<(), Outcome> {
    if let Some(rebuild_command) = &config.rebuild_command {
        let (program, arguments) = rebuild_command
            .split_first()
            .ok_or_else(|| Outcome::Failed(None, "rebuild_command is empty".into()))?;
        let mut command = Command::new(program);
        command.args(arguments).arg(&config.host).arg(commit);
        return log.run("rebuild command", &mut command);
    }

    let flake_attr = config.flake_attr();
    let fail = |message: String| Outcome::Failed(None, message);
    log.line("nix build");
    let mut build = git
        .user_command("nix")
        .map_err(|error| fail(error.to_string()))?;
    build
        .args(["build", "--no-link", "--print-out-paths"])
        .arg(format!("{}#{flake_attr}", config.flake_path.display()))
        .stdin(Stdio::null())
        .stderr(log.stdio().map_err(|error| fail(error.to_string()))?.1);
    let output = build
        .output()
        .map_err(|error| fail(format!("nix build: {error}")))?;
    let _ = log.0.write_all(&output.stdout);
    log.finish("nix build", output.status)?;
    let store_path = String::from_utf8_lossy(&output.stdout)
        .lines()
        .last()
        .unwrap_or("")
        .trim()
        .to_string();
    if !store_path.starts_with("/nix/store/") {
        return Err(fail("nix build printed no store path".into()));
    }

    log.run(
        "nix-env --set",
        Command::new("nix-env").args(["-p", SYSTEM_PROFILE, "--set", &store_path]),
    )?;
    let (what, mut switch) = if config.os == "darwin" {
        ("activate", Command::new(format!("{store_path}/activate")))
    } else {
        let mut command = Command::new(format!("{store_path}/bin/switch-to-configuration"));
        command.arg("switch").env_remove("NIXOS_INSTALL_BOOTLOADER");
        ("switch-to-configuration", command)
    };
    log.run(what, &mut switch)
}

fn steps(config: &AgentConfig, commit: &str, log: &mut ApplyLog) -> Outcome {
    let git = match Git::new(config) {
        Ok(git) => git,
        Err(error) => return Outcome::Failed(None, format!("{error:#}")),
    };
    log.line("git fetch");
    if let Err(error) = git.fetch(&config.repo_url) {
        let message = format!("{error:#}");
        log.line(&message);
        return Outcome::Skipped(SkipReason::FetchFailed, message);
    }
    record_fetch(config);
    log.line("checks");
    if let Err(reason) = check_apply(&git, &config.channel, commit) {
        let message = format!("skipped: {reason:?}");
        log.line(&message);
        return Outcome::Skipped(reason, message);
    }
    log.line("git merge --ff-only");
    if let Err(error) = git.ff_merge(commit) {
        let message = format!("{error:#}");
        log.line(&message);
        return Outcome::Failed(None, message);
    }
    match build_and_switch(config, &git, commit, log) {
        Ok(()) => Outcome::Ok,
        Err(outcome) => outcome,
    }
}

/// The wrapper body: runs the whole apply, keeping `<id>.json` current.
pub fn run_apply(config: &AgentConfig, id: &str, commit: &str) -> ApplyRecord {
    let mut record = ApplyRecord {
        apply_id: id.to_string(),
        commit: commit.to_string(),
        state: ApplyState::Failed,
        skip_reason: None,
        message: None,
        exit_code: None,
        started_at: Some(now()),
        finished_at: None,
    };
    if !is_valid_apply_id(id) {
        record.message = Some("invalid apply id".into());
        return record;
    }
    // Held until the function returns; keeps a second wrapper out.
    let Some(_lock) = lock_apply_briefly(config) else {
        eprintln!("lodestar-agent: another apply is running; not starting {id}");
        record.message = Some("another apply is running".into());
        return record;
    };
    if let Err(error) = start(config, &mut record) {
        record.message = Some(format!("{error:#}"));
        record.finished_at = Some(now());
        return record;
    }
    let in_progress = in_progress_marker(config);

    let outcome = if !is_commit_hash(commit) {
        Outcome::Failed(None, "invalid commit".into())
    } else {
        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(apply_file(config, id, "log"))
        {
            Ok(log_file) => steps(config, commit, &mut ApplyLog(log_file)),
            Err(error) => Outcome::Failed(None, format!("opening log: {error}")),
        }
    };
    match outcome {
        Outcome::Ok => {
            record.state = ApplyState::Ok;
            record.exit_code = Some(0);
        }
        Outcome::Skipped(reason, message) => {
            record.state = ApplyState::Skipped;
            record.skip_reason = Some(reason);
            record.message = Some(message);
        }
        Outcome::Failed(code, message) => {
            record.state = ApplyState::Failed;
            record.exit_code = code;
            record.message = Some(message);
        }
    }
    record.finished_at = Some(now());
    if let Err(error) = write_record(config, &record) {
        eprintln!("lodestar-agent: {error:#}");
    }
    let _ = fs::remove_file(in_progress);
    let _ = fs::remove_file(apply_file(config, id, "pid"));
    prune(config, 50);
    record
}

/// Step 1: pid file, in-progress marker and the `Running` record.
fn start(config: &AgentConfig, record: &mut ApplyRecord) -> anyhow::Result<()> {
    fs::create_dir_all(applies_dir(config)).context("creating applies dir")?;
    // The pid goes first, so a `Running` record is never seen without it.
    fs::write(
        apply_file(config, &record.apply_id, "pid"),
        std::process::id().to_string(),
    )
    .context("writing pid file")?;
    let marker = serde_json::json!({
        "apply_id": record.apply_id,
        "commit": record.commit,
        "started": record.started_at,
    });
    write_atomic(&in_progress_marker(config), marker.to_string().as_bytes())
        .context("writing apply-in-progress.json")?;
    record.state = ApplyState::Running;
    write_record(config, record)?;
    Ok(())
}
