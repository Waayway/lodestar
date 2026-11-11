//! What the build gate asks of the builder, and the real implementation that builds on a
//! remote store over SSH and publishes there with `ssh`.

use std::collections::{BTreeMap, HashSet};
use std::ffi::OsString;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::Duration;

use async_trait::async_trait;
use lodestar_proto::is_commit_hash;
use tokio::process::Command;
use tokio::time::Instant;

use crate::config::{ManagedHost, ServerConfig, valid_host_name};
use crate::flake_lock::{COMMITTED_LOCK, FlakeLock, RESTRICTED_EVALUATION};
use crate::nix_sandbox::{Access, NixSandbox};
use crate::process::{RunError, log_tail, run};

/// Prefix of every failure reason caused by not reaching the builder (the gate relies on it).
const UNREACHABLE: &str = "builder unreachable: ";
/// ssh's own exit status for connection errors.
const SSH_CONNECT_FAILED: i32 = 255;
/// How long a reachability or path probe of the builder store may take.
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);
/// Nix's base-32 alphabet, which store path hashes use.
const NIX_BASE32: &str = "0123456789abcdfghijklmnpqrsvwxyz";

/// Whether `build` failed because the builder could not be reached (its store did not answer
/// after the build failed), not because of the commit.
pub(crate) fn is_unreachable(build: &HostBuild) -> bool {
    !build.ok
        && build
            .log_tail
            .as_deref()
            .is_some_and(|reason| reason.starts_with(UNREACHABLE))
}

/// The outcome of building one host's system.
#[derive(Clone, Debug, PartialEq)]
pub struct HostBuild {
    pub ok: bool,
    /// The system toplevel's store path, when it was built.
    pub path: Option<String>,
    /// Why it failed: the last 40 lines of the log.
    pub log_tail: Option<String>,
}

impl HostBuild {
    fn built(path: String) -> Self {
        Self {
            ok: true,
            path: Some(path),
            log_tail: None,
        }
    }

    fn failed(reason: impl Into<String>) -> Self {
        Self {
            ok: false,
            path: None,
            log_tail: Some(reason.into()),
        }
    }
}

/// Builds and publishes on the builder (tests inject a fake).
#[async_trait]
pub trait Builder: Send + Sync {
    /// Builds the toplevel of every host in `hosts` from the flake in `dir` on the builder store.
    async fn build(&self, dir: &Path, hosts: &[ManagedHost]) -> BTreeMap<String, HostBuild>;
    /// Runs the publish command on the builder: `<cmd…> <commit> <path>…`; `channels` JSON on stdin.
    async fn publish(
        &self,
        commit: &str,
        paths: &[String],
        channels: &serde_json::Value,
    ) -> anyhow::Result<()>;
}

/// Builds with `nix build --store <store>` and publishes with `ssh <store's host>`.
pub struct RemoteBuilder {
    pub nix_bin: PathBuf,
    pub ssh_bin: PathBuf,
    /// `ssh-ng://[user@]host[:port]` (or `ssh://`).
    pub store: String,
    pub ssh_key_file: Option<PathBuf>,
    pub publish_command: Vec<String>,
    /// For a whole build (all evaluations and the build) and for a publish.
    pub timeout: Duration,
    /// Confines every `nix` it runs.
    pub sandbox: NixSandbox,
}

/// The SSH destination and port of a store URL.
struct Destination {
    target: String,
    port: Option<String>,
}

/// `/nix/store/<32 base-32 chars>-<name>` with only `[A-Za-z0-9+._=-]` in the name (not
/// starting with `.`), so the remote shell that ssh hands the command to reads it as one plain
/// word.
fn valid_store_path(path: &str) -> bool {
    let Some((hash, name)) = path
        .strip_prefix("/nix/store/")
        .and_then(|rest| rest.split_once('-'))
    else {
        return false;
    };
    hash.len() == 32
        && hash.chars().all(|c| NIX_BASE32.contains(c))
        && !name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.' | '_' | '='))
}

/// Runs `command` for at most `limit`; a timeout is reported as `reported` (the budget the
/// caller set, of which `limit` may be the rest).
async fn run_limited(
    program: &Path,
    command: &mut Command,
    stdin: Option<&[u8]>,
    limit: Duration,
    reported: Duration,
) -> Result<Output, String> {
    run(command, stdin, limit)
        .await
        .map_err(|error| match error {
            RunError::Start(error) => format!("starting {}: {error}", program.display()),
            RunError::Wait(error) => format!("running {}: {error}", program.display()),
            RunError::Timeout => format!(
                "{} timed out after {}s",
                program.display(),
                reported.as_secs()
            ),
        })
}

/// The stderr of a failed run (its first errors and its tail), or its exit status when it
/// logged nothing.
fn failure(program: &str, output: &Output) -> String {
    let excerpt = log_excerpt(&String::from_utf8_lossy(&output.stderr));
    if excerpt.is_empty() {
        format!("{program} failed ({})", output.status)
    } else {
        excerpt
    }
}

/// At most this many lines of root-cause errors go before the tail.
const ROOT_CAUSE_LINES: usize = 80;

/// The errors in `text` that are not about a failed dependency (with their indented lines, up
/// to 80 lines), then the last 40 lines. Nix reports the real error once and then one "Cannot
/// build … dependency failed" per dependent, which pushes it out of a plain tail.
fn log_excerpt(text: &str) -> String {
    let tail = log_tail(text);
    let mut causes: Vec<&str> = Vec::new();
    let mut in_cause = false;
    for line in text.lines() {
        if let Some(message) = line.strip_prefix("error:") {
            in_cause = !(message.trim_start().starts_with("Cannot build")
                || message.contains("dependency failed")
                || message.contains("dependencies failed"));
        } else if !line.starts_with(char::is_whitespace) {
            in_cause = false;
        }
        if in_cause {
            causes.push(line);
        }
    }
    causes.truncate(ROOT_CAUSE_LINES);
    let causes = causes.join("\n");
    if causes.is_empty() || tail.contains(&causes) {
        tail
    } else {
        format!("{causes}\n…\n{tail}")
    }
}

impl RemoteBuilder {
    /// The builder `config.builds` describes, with the server's `nix`.
    pub fn from_config(config: &ServerConfig) -> Self {
        Self {
            nix_bin: config.nix_bin.clone(),
            ssh_bin: PathBuf::from("ssh"),
            store: config.builds.store.clone(),
            ssh_key_file: config.builds.ssh_key_file.clone(),
            publish_command: config.builds.publish_command.clone(),
            timeout: Duration::from_secs(config.builds.timeout_secs),
            sandbox: NixSandbox::new(config.state_dir.join("nix-sandbox")),
        }
    }

    /// The authority of `store`: `[user@]host[:port]`, checked so ssh can't read it as an option.
    fn destination(&self) -> Result<Destination, String> {
        let invalid = || format!("invalid builder store {:?}", self.store);
        let authority = self
            .store
            .strip_prefix("ssh-ng://")
            .or_else(|| self.store.strip_prefix("ssh://"))
            .ok_or_else(invalid)?;
        let (target, port) = match authority.rsplit_once(':') {
            Some((target, port)) => (target, Some(port)),
            None => (authority, None),
        };
        let host = target.rsplit_once('@').map_or(target, |(_, host)| host);
        let plain = |text: &str| {
            !text.is_empty()
                && !text.starts_with('-')
                && text
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '@'))
        };
        if !plain(target)
            || !plain(host)
            || target.matches('@').count() > 1
            || port.is_some_and(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_digit()))
        {
            return Err(invalid());
        }
        Ok(Destination {
            target: target.to_string(),
            port: port.map(str::to_string),
        })
    }

    /// The ssh options for the builder, for `ssh` itself and for `NIX_SSHOPTS`.
    fn ssh_options(&self) -> Vec<OsString> {
        let mut options = Vec::new();
        if let Some(key) = &self.ssh_key_file {
            options.push("-i".into());
            options.push(key.clone().into_os_string());
        }
        for option in [
            "IdentitiesOnly=yes",
            "StrictHostKeyChecking=accept-new",
            "BatchMode=yes",
            "ConnectTimeout=10",
        ] {
            options.push("-o".into());
            options.push(option.into());
        }
        options
    }

    /// `nix --extra-experimental-features 'nix-command flakes' <RESTRICTED_EVALUATION> <args>`
    /// in `dir`, confined for `access`; the ruleset must live until the command ran.
    fn nix(
        &self,
        dir: &Path,
        args: &[&str],
        access: Access<'_>,
    ) -> Result<(Command, OwnedFd), String> {
        let mut command = Command::new(&self.nix_bin);
        command
            .arg("--extra-experimental-features")
            .arg("nix-command flakes")
            .args(RESTRICTED_EVALUATION)
            .args(args)
            .current_dir(dir);
        let ruleset = self
            .sandbox
            .confine(&mut command, dir, access)
            .map_err(|error| format!("{error:#}"))?;
        Ok((command, ruleset))
    }

    /// What a command that talks to the builder may read besides the evaluation's needs.
    fn builder_access(&self) -> Access<'_> {
        Access::Builder {
            key: self.ssh_key_file.as_deref(),
        }
    }

    /// Runs `command` until `deadline` (part of the whole `timeout`); the output, or why there
    /// is none.
    async fn run_until(
        &self,
        program: &Path,
        command: &mut Command,
        stdin: Option<&[u8]>,
        deadline: Instant,
    ) -> Result<Output, String> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        run_limited(program, command, stdin, remaining, self.timeout).await
    }

    /// Whether the builder store answers (`nix store info`); why not, when it doesn't.
    async fn probe(&self, dir: &Path) -> Result<(), String> {
        let (mut command, _ruleset) = self.nix(
            dir,
            &["store", "info", "--store", &self.store],
            self.builder_access(),
        )?;
        command.env("NIX_SSHOPTS", self.nix_sshopts());
        let output = run_limited(
            &self.nix_bin,
            &mut command,
            None,
            PROBE_TIMEOUT,
            PROBE_TIMEOUT,
        )
        .await?;
        if output.status.success() {
            Ok(())
        } else {
            Err(failure("nix store info", &output))
        }
    }

    /// The toplevel `outPath` of `host`, evaluated locally.
    async fn out_path(
        &self,
        dir: &Path,
        attribute: &str,
        deadline: Instant,
    ) -> Result<String, String> {
        let attribute = format!("{attribute}.outPath");
        let (mut command, _ruleset) = self.nix(
            dir,
            &["eval", "--raw", COMMITTED_LOCK, &attribute],
            Access::Evaluate,
        )?;
        let output = self
            .run_until(&self.nix_bin, &mut command, None, deadline)
            .await?;
        if !output.status.success() {
            return Err(failure("nix eval", &output));
        }
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if valid_store_path(&path) {
            Ok(path)
        } else {
            Err(format!("nix eval printed {path:?}, not a store path"))
        }
    }

    /// The toplevel `drvPath` of `host`, evaluated locally. Realising this later needs no
    /// flake evaluation at all, so it never has to run with `Access::Builder` (which, for the
    /// builder's own SSH key, is exactly the access an evaluation of attacker-controlled Nix
    /// code must never have).
    async fn drv_path(
        &self,
        dir: &Path,
        attribute: &str,
        deadline: Instant,
    ) -> Result<String, String> {
        let attribute = format!("{attribute}.drvPath");
        let (mut command, _ruleset) = self.nix(
            dir,
            &["eval", "--raw", COMMITTED_LOCK, &attribute],
            Access::Evaluate,
        )?;
        let output = self
            .run_until(&self.nix_bin, &mut command, None, deadline)
            .await?;
        if !output.status.success() {
            return Err(failure("nix eval", &output));
        }
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if valid_store_path(&path) {
            Ok(path)
        } else {
            Err(format!("nix eval printed {path:?}, not a derivation path"))
        }
    }

    /// Whether `path` is valid in the builder store (built earlier, or built although a failure
    /// of another host made `nix build` exit without printing it). Gets at least
    /// `PROBE_TIMEOUT`, even when a long build used up the deadline.
    async fn on_store(&self, dir: &Path, path: &str, deadline: Instant) -> bool {
        let Ok((mut command, _ruleset)) = self.nix(
            dir,
            &["path-info", "--store", &self.store, path],
            self.builder_access(),
        ) else {
            return false;
        };
        command.env("NIX_SSHOPTS", self.nix_sshopts());
        let limit = deadline
            .saturating_duration_since(Instant::now())
            .max(PROBE_TIMEOUT);
        run_limited(&self.nix_bin, &mut command, None, limit, limit)
            .await
            .is_ok_and(|output| output.status.success())
    }

    fn nix_sshopts(&self) -> OsString {
        self.ssh_options().join(std::ffi::OsStr::new(" "))
    }
}

#[async_trait]
impl Builder for RemoteBuilder {
    async fn build(&self, dir: &Path, hosts: &[ManagedHost]) -> BTreeMap<String, HostBuild> {
        let deadline = Instant::now() + self.timeout;
        let mut results = BTreeMap::new();
        let mut candidates = Vec::new();
        for host in hosts {
            if !valid_host_name(&host.name) {
                results.insert(
                    host.name.clone(),
                    HostBuild::failed(format!("invalid host name {:?}", host.name)),
                );
            } else if host.os == "darwin" {
                results.insert(
                    host.name.clone(),
                    HostBuild::failed("darwin is not built here"),
                );
            } else {
                candidates.push(host);
            }
        }
        if candidates.is_empty() {
            return results;
        }
        if let Err(reason) = self.destination() {
            for host in candidates {
                results.insert(host.name.clone(), HostBuild::failed(reason.clone()));
            }
            return results;
        }
        // Nothing of the channel's flake is evaluated while its lock names a local source.
        if let Err(error) = FlakeLock::check_dir(dir) {
            let reason = format!("{error:#}");
            for host in candidates {
                results.insert(host.name.clone(), HostBuild::failed(reason.clone()));
            }
            return results;
        }
        // (host name, toplevel attribute, expected out path, derivation path to realise).
        let mut wanted: Vec<(String, String, String, String)> = Vec::new();
        for host in candidates {
            let attribute = format!(
                ".#nixosConfigurations.\"{}\".config.system.build.toplevel",
                host.name
            );
            match self.out_path(dir, &attribute, deadline).await {
                Ok(path) => match self.drv_path(dir, &attribute, deadline).await {
                    Ok(drv) => wanted.push((host.name.clone(), attribute, path, drv)),
                    Err(reason) => {
                        results.insert(host.name.clone(), HostBuild::failed(reason));
                    }
                },
                Err(reason) => {
                    results.insert(host.name.clone(), HostBuild::failed(reason));
                }
            }
        }
        if wanted.is_empty() {
            return results;
        }

        // Every derivation is already fixed by the evaluations above, so realising it needs
        // no flake evaluation at all: `Access::Builder` (which exposes the builder's SSH key)
        // never coexists with evaluation of the channel's Nix code.
        let targets: Vec<String> = wanted
            .iter()
            .map(|(_, _, _, drv)| format!("{drv}^*"))
            .collect();
        let mut args = vec!["build", "--no-link", "--print-out-paths", "--keep-going"];
        args.push("--store");
        args.push(&self.store);
        args.extend(targets.iter().map(String::as_str));
        let output = match self.nix(dir, &args, self.builder_access()) {
            Ok((mut command, _ruleset)) => {
                command.env("NIX_SSHOPTS", self.nix_sshopts());
                self.run_until(&self.nix_bin, &mut command, None, deadline)
                    .await
            }
            Err(reason) => Err(reason),
        };
        let output = match output {
            Ok(output) => output,
            Err(reason) => {
                for (name, _, _, _) in wanted {
                    results.insert(name, HostBuild::failed(reason.clone()));
                }
                return results;
            }
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        let printed: HashSet<&str> = stdout.lines().map(str::trim).collect();
        // Build logs may contain any text (a test suite's "Connection refused"), so whether the
        // builder was reachable is asked of the store, never read from the log.
        if !output.status.success()
            && let Err(probe) = self.probe(dir).await
        {
            let reason = format!("{UNREACHABLE}{probe}");
            for (name, _, _, _) in wanted {
                results.insert(name, HostBuild::failed(reason.clone()));
            }
            return results;
        }
        let reason = failure("nix build", &output);
        for (name, _, path, _) in wanted {
            let built =
                printed.contains(path.as_str()) || self.on_store(dir, &path, deadline).await;
            let result = if built {
                HostBuild::built(path)
            } else {
                HostBuild::failed(reason.clone())
            };
            results.insert(name, result);
        }
        results
    }

    async fn publish(
        &self,
        commit: &str,
        paths: &[String],
        channels: &serde_json::Value,
    ) -> anyhow::Result<()> {
        if !is_commit_hash(commit) {
            anyhow::bail!("invalid commit {commit:?}");
        }
        if let Some(path) = paths.iter().find(|path| !valid_store_path(path)) {
            anyhow::bail!("not a store path: {path:?}");
        }
        let Some(program) = self.publish_command.first().filter(|p| !p.is_empty()) else {
            anyhow::bail!("builds.publish_command must name a program");
        };
        let destination = self.destination().map_err(anyhow::Error::msg)?;
        let stdin = serde_json::to_vec(channels)?;

        let mut command = Command::new(&self.ssh_bin);
        command.args(self.ssh_options());
        if let Some(port) = &destination.port {
            command.arg("-p").arg(port);
        }
        command
            .arg("--")
            .arg(&destination.target)
            .arg(program)
            .args(&self.publish_command[1..])
            .arg(commit)
            .args(paths);
        let deadline = Instant::now() + self.timeout;
        let output = self
            .run_until(&self.ssh_bin, &mut command, Some(&stdin), deadline)
            .await
            .map_err(anyhow::Error::msg)?;
        if output.status.success() {
            return Ok(());
        }
        let reason = failure(program, &output);
        // ssh exits 255 when it cannot connect; a remote command that itself exits 255 is
        // reported as unreachable too.
        if output.status.code() == Some(SSH_CONNECT_FAILED) {
            anyhow::bail!("{UNREACHABLE}{reason}");
        }
        anyhow::bail!("publishing {commit} failed: {reason}")
    }
}

#[cfg(test)]
mod tests {
    use super::log_excerpt;

    #[test]
    fn the_root_cause_comes_before_the_dependency_noise() {
        let mut log = String::from(
            "building '/nix/store/a-dep.drv'...\n\
             error: builder for '/nix/store/a-dep.drv' failed with exit code 101;\n\
             \x20      last 2 log lines:\n\
             \x20      > error[E0425]: cannot find value `x`\n\
             \x20      > aborting\n",
        );
        for n in 0..60 {
            log.push_str(&format!(
                "error: Cannot build '/nix/store/{n}-x.drv'.\n       Reason: 1 dependency failed.\n"
            ));
        }
        let excerpt = log_excerpt(&log);
        assert!(excerpt.starts_with("error: builder for '/nix/store/a-dep.drv' failed"));
        assert!(excerpt.contains("cannot find value `x`"));
        assert!(excerpt.contains("\n…\n"));
        assert!(excerpt.ends_with("Reason: 1 dependency failed."));
        assert!(!excerpt.contains("building '/nix/store/a-dep.drv'"));
    }

    #[test]
    fn a_short_log_is_its_tail() {
        let log = "copying path\nerror: path '/nix/store/x' is not valid\n";
        assert_eq!(
            log_excerpt(log),
            "copying path\nerror: path '/nix/store/x' is not valid"
        );
    }

    #[test]
    fn only_dependency_errors_give_the_tail() {
        let log = "error: Cannot build '/nix/store/x.drv'.\n       Reason: 1 dependency failed.\n";
        assert_eq!(log_excerpt(log), log.trim_end());
    }
}
