//! What the updater asks of Nix, and the real implementation that runs the `nix` binary.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::Context;
use async_trait::async_trait;

use crate::config::ManagedHost;

/// Builds may take long on a small builder.
const BUILD_TIMEOUT: Duration = Duration::from_secs(2 * 60 * 60);
const EVAL_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const LOG_TAIL_LINES: usize = 40;

/// Nix operations on a worktree of the config flake (tests inject a fake).
#[async_trait]
pub trait NixRunner: Send + Sync {
    /// `nix flake update <input>` in `dir`.
    async fn flake_update(&self, dir: &Path, input: &str) -> anyhow::Result<()>;
    /// `services.lodestar-agent.versions` of `host` in the flake at `dir`.
    async fn eval_versions(
        &self,
        dir: &Path,
        host: &ManagedHost,
    ) -> anyhow::Result<BTreeMap<String, String>>;
    /// Linux: builds the system toplevel; darwin: evaluates the system derivation. `Err` is
    /// the tail of the log (40 lines).
    async fn build(&self, dir: &Path, host: &ManagedHost) -> Result<(), String>;
}

/// Runs `nix --extra-experimental-features 'nix-command flakes' …` in the worktree.
pub struct NixCommand {
    pub bin: PathBuf,
}

/// The flake attribute set of `host`'s configuration, with the host name quoted so a `.`
/// in it is not read as an attribute separator. The name must already be validated.
fn configuration(host: &ManagedHost) -> String {
    let kind = if host.os == "darwin" {
        "darwinConfigurations"
    } else {
        "nixosConfigurations"
    };
    format!(".#{kind}.\"{}\"", host.name)
}

/// The last `LOG_TAIL_LINES` lines of `text`.
pub fn log_tail(text: &str) -> String {
    let lines: Vec<&str> = text.trim_end().lines().collect();
    lines[lines.len().saturating_sub(LOG_TAIL_LINES)..].join("\n")
}

/// Whether the root node of the lock file at `path` has a direct input `input`.
fn lock_has_input(path: &Path, input: &str) -> anyhow::Result<bool> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let lock: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    let root = lock["root"].as_str().unwrap_or("root");
    Ok(lock["nodes"][root]["inputs"].get(input).is_some())
}

impl NixCommand {
    /// stdout on success; the stderr tail (or why it did not run) on failure.
    async fn run(&self, dir: &Path, args: &[&OsStr], timeout: Duration) -> Result<String, String> {
        let child = tokio::process::Command::new(&self.bin)
            .arg("--extra-experimental-features")
            .arg("nix-command flakes")
            .args(args)
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| format!("starting {}: {error}", self.bin.display()))?;
        let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(error)) => return Err(format!("running {}: {error}", self.bin.display())),
            Err(_) => return Err(format!("nix timed out after {}s", timeout.as_secs())),
        };
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
        }
        let tail = log_tail(&String::from_utf8_lossy(&output.stderr));
        Err(if tail.is_empty() {
            format!("nix failed ({})", output.status)
        } else {
            tail
        })
    }
}

#[async_trait]
impl NixRunner for NixCommand {
    async fn flake_update(&self, dir: &Path, input: &str) -> anyhow::Result<()> {
        if input.starts_with('-') {
            anyhow::bail!("invalid input name {input:?}");
        }
        // `nix flake update <unknown>` only warns; without this check a misspelt input would
        // read as "no change" forever.
        if !lock_has_input(&dir.join("flake.lock"), input)? {
            anyhow::bail!("flake.lock has no input {input:?}");
        }
        self.run(
            dir,
            &[OsStr::new("flake"), OsStr::new("update"), OsStr::new(input)],
            EVAL_TIMEOUT,
        )
        .await
        .map(drop)
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("nix flake update {input}"))
    }

    async fn eval_versions(
        &self,
        dir: &Path,
        host: &ManagedHost,
    ) -> anyhow::Result<BTreeMap<String, String>> {
        // A configuration whose agent module predates the `versions` option tracks nothing;
        // any other evaluation error is an error.
        let attribute = format!("{}.config.services.lodestar-agent", configuration(host));
        let stdout = self
            .run(
                dir,
                &[
                    OsStr::new("eval"),
                    OsStr::new("--json"),
                    OsStr::new("--no-write-lock-file"),
                    OsStr::new(&attribute),
                    OsStr::new("--apply"),
                    OsStr::new("agent: agent.versions or { }"),
                ],
                EVAL_TIMEOUT,
            )
            .await
            .map_err(anyhow::Error::msg)
            .with_context(|| format!("evaluating the versions of {}", host.name))?;
        serde_json::from_str(&stdout)
            .with_context(|| format!("the versions of {} are not a map of strings", host.name))
    }

    async fn build(&self, dir: &Path, host: &ManagedHost) -> Result<(), String> {
        let configuration = configuration(host);
        if host.os == "darwin" {
            // Can't be built here; evaluating the system derivation catches most breakage.
            let attribute = format!("{configuration}.system.drvPath");
            self.run(
                dir,
                &[
                    OsStr::new("eval"),
                    OsStr::new("--raw"),
                    OsStr::new("--no-write-lock-file"),
                    OsStr::new(&attribute),
                ],
                EVAL_TIMEOUT,
            )
            .await
            .map(drop)
        } else {
            let attribute = format!("{configuration}.config.system.build.toplevel");
            self.run(
                dir,
                &[
                    OsStr::new("build"),
                    OsStr::new("--no-link"),
                    OsStr::new("--no-write-lock-file"),
                    OsStr::new(&attribute),
                ],
                BUILD_TIMEOUT,
            )
            .await
            .map(drop)
        }
    }
}
