#![allow(dead_code)]
use std::path::{Path, PathBuf};
use std::process::Command;

use lodestar_agent::config::{AgentConfig, Launcher};
use tempfile::TempDir;

pub struct Fixture {
    pub _tmp: TempDir,
    pub remote: PathBuf,
    pub clone: PathBuf,
    pub config: AgentConfig,
}

/// Runs git with a fixed identity and no global/system config.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

pub fn commit_file(dir: &Path, name: &str, content: &str) -> String {
    std::fs::write(dir.join(name), content).unwrap();
    git(dir, &["add", name]);
    git(dir, &["commit", "-q", "-m", name]);
    git(dir, &["rev-parse", "HEAD"])
}

/// Bare remote with `unstable` and `stable`, plus a clone on `unstable`.
pub fn fixture() -> Fixture {
    let temporary_dir = TempDir::new().unwrap();
    let remote = temporary_dir.path().join("remote.git");
    let seed = temporary_dir.path().join("seed");
    let clone = temporary_dir.path().join("clone");
    std::fs::create_dir_all(&remote).unwrap();
    std::fs::create_dir_all(&seed).unwrap();
    git(&remote, &["init", "-q", "--bare", "-b", "unstable"]);
    git(&seed, &["init", "-q", "-b", "unstable"]);
    commit_file(&seed, "a.txt", "a");
    git(&seed, &["branch", "stable"]);
    git(
        &seed,
        &["push", "-q", remote.to_str().unwrap(), "unstable", "stable"],
    );
    git(
        temporary_dir.path(),
        &[
            "clone",
            "-q",
            "-b",
            "unstable",
            remote.to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    let config = AgentConfig {
        host: "h".into(),
        user: std::env::var("USER").unwrap_or_else(|_| "root".into()),
        channel: "unstable".into(),
        flake_path: clone.clone(),
        flake_attr: None,
        repo_url: remote.to_string_lossy().into_owned(),
        port: 7461,
        manager_host: "m".into(),
        listen: None,
        state_dir: temporary_dir.path().join("state"),
        rebuild_command: None,
        revision_command: None,
        launcher: Launcher::Inline,
        agent_exe: None,
        os: "linux".into(),
    };
    Fixture {
        _tmp: temporary_dir,
        remote,
        clone,
        config,
    }
}

/// Adds a commit to the remote's `unstable` via a scratch clone; returns its hash.
pub fn push_remote_commit(fixture: &Fixture, name: &str) -> String {
    let work_dir = fixture._tmp.path().join(format!("w-{name}"));
    git(
        fixture._tmp.path(),
        &[
            "clone",
            "-q",
            "-b",
            "unstable",
            fixture.remote.to_str().unwrap(),
            work_dir.to_str().unwrap(),
        ],
    );
    let commit = commit_file(&work_dir, name, name);
    git(&work_dir, &["push", "-q", "origin", "unstable"]);
    commit
}
