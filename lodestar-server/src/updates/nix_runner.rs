//! What the updater asks of Nix, and the real implementation that runs the `nix` binary.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use async_trait::async_trait;

use crate::config::ManagedHost;
use crate::flake_lock::{COMMITTED_LOCK, FlakeLock, RESTRICTED_EVALUATION};
use crate::nix_sandbox::{Access, NixSandbox};
pub use crate::process::log_tail;
use crate::process::{RunError, run};

/// Builds may take long on a small builder.
const BUILD_TIMEOUT: Duration = Duration::from_secs(2 * 60 * 60);
const EVAL_TIMEOUT: Duration = Duration::from_secs(10 * 60);

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

/// Runs `nix --extra-experimental-features 'nix-command flakes' …` in the worktree, confined
/// by `sandbox`.
pub struct NixCommand {
    pub bin: PathBuf,
    pub sandbox: NixSandbox,
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

/// The lock after `nix flake update <input>`: remote sources only, and only `input` moved.
fn check_updated(dir: &Path, old: &FlakeLock, input: &str) -> anyhow::Result<()> {
    let Some(new) = FlakeLock::read(dir)? else {
        anyhow::bail!("flake.lock is missing after the update");
    };
    new.check_sources()?;
    old.check_update(&new, input)
}

impl NixCommand {
    /// stdout on success; the stderr tail (or why it did not run) on failure. Every command
    /// runs with `RESTRICTED_EVALUATION`, confined for `access`.
    async fn run(
        &self,
        dir: &Path,
        args: &[&OsStr],
        access: Access<'_>,
        timeout: Duration,
    ) -> Result<String, String> {
        let mut command = tokio::process::Command::new(&self.bin);
        command
            .arg("--extra-experimental-features")
            .arg("nix-command flakes")
            .args(RESTRICTED_EVALUATION)
            .args(args)
            .current_dir(dir);
        let _ruleset = self
            .sandbox
            .confine(&mut command, dir, access)
            .map_err(|error| format!("{error:#}"))?;
        let output = match run(&mut command, None, timeout).await {
            Ok(output) => output,
            Err(RunError::Start(error)) => {
                return Err(format!("starting {}: {error}", self.bin.display()));
            }
            Err(RunError::Wait(error)) => {
                return Err(format!("running {}: {error}", self.bin.display()));
            }
            Err(RunError::Timeout) => {
                return Err(format!("nix timed out after {}s", timeout.as_secs()));
            }
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
        let path = dir.join("flake.lock");
        let Some(old) = FlakeLock::read(dir)? else {
            anyhow::bail!("reading {}: no such file", path.display());
        };
        // `nix flake update <unknown>` only warns; without this check a misspelt input would
        // read as "no change" forever.
        if !old.has_input(input) {
            anyhow::bail!("flake.lock has no input {input:?}");
        }
        old.check_sources()?;
        let old_text =
            std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        self.run(
            dir,
            &[OsStr::new("flake"), OsStr::new("update"), OsStr::new(input)],
            Access::Update,
            EVAL_TIMEOUT,
        )
        .await
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("nix flake update {input}"))?;
        // Nix also locks any input the lock lacks (one the channel added to `flake.nix`): such
        // a lock is put back before anything evaluates it.
        if let Err(error) = check_updated(dir, &old, input) {
            std::fs::write(&path, &old_text)
                .with_context(|| format!("restoring {}", path.display()))?;
            return Err(error.context(format!("nix flake update {input}")));
        }
        Ok(())
    }

    async fn eval_versions(
        &self,
        dir: &Path,
        host: &ManagedHost,
    ) -> anyhow::Result<BTreeMap<String, String>> {
        // A configuration whose agent module predates the `versions` option tracks nothing;
        // any other evaluation error is an error.
        let attribute = format!("{}.config.services.lodestar-agent", configuration(host));
        FlakeLock::check_dir(dir)
            .with_context(|| format!("evaluating the versions of {}", host.name))?;
        let stdout = self
            .run(
                dir,
                &[
                    OsStr::new("eval"),
                    OsStr::new("--json"),
                    OsStr::new(COMMITTED_LOCK),
                    OsStr::new(&attribute),
                    OsStr::new("--apply"),
                    OsStr::new("agent: agent.versions or { }"),
                ],
                Access::Evaluate,
                EVAL_TIMEOUT,
            )
            .await
            .map_err(anyhow::Error::msg)
            .with_context(|| format!("evaluating the versions of {}", host.name))?;
        serde_json::from_str(&stdout)
            .with_context(|| format!("the versions of {} are not a map of strings", host.name))
    }

    async fn build(&self, dir: &Path, host: &ManagedHost) -> Result<(), String> {
        FlakeLock::check_dir(dir).map_err(|error| format!("{error:#}"))?;
        let configuration = configuration(host);
        if host.os == "darwin" {
            // Can't be built here; evaluating the system derivation catches most breakage.
            let attribute = format!("{configuration}.system.drvPath");
            self.run(
                dir,
                &[
                    OsStr::new("eval"),
                    OsStr::new("--raw"),
                    OsStr::new(COMMITTED_LOCK),
                    OsStr::new(&attribute),
                ],
                Access::Evaluate,
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
                    OsStr::new(COMMITTED_LOCK),
                    OsStr::new(&attribute),
                ],
                Access::Evaluate,
                BUILD_TIMEOUT,
            )
            .await
            .map(drop)
        }
    }
}
