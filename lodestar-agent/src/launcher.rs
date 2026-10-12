//! Starts the apply wrapper outside the agent daemon.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, bail};
use lodestar_proto::is_commit_hash;

use crate::apply::is_valid_apply_id;
use crate::config::{AgentConfig, Launcher};

/// The daemon PATH (global constraints); transient units do not inherit it.
pub const DAEMON_PATH: &str =
    "/run/current-system/sw/bin:/nix/var/nix/profiles/default/bin:/usr/bin:/bin:/usr/sbin:/sbin";

/// Root's home directory for the wrapper: `/var/root` on macOS, `/root` elsewhere.
pub fn daemon_home(config: &AgentConfig) -> &'static str {
    if config.os == "darwin" {
        "/var/root"
    } else {
        "/root"
    }
}

fn exe_path(config: &AgentConfig) -> anyhow::Result<PathBuf> {
    match &config.agent_exe {
        Some(path) => Ok(path.clone()),
        None => std::env::current_exe().context("locating own executable"),
    }
}

/// Program and argv for starting the wrapper. Pure, so it can be unit-tested.
/// `--` keeps a hostile positional from being parsed as a flag.
pub fn launch_command(
    config: &AgentConfig,
    config_path: &Path,
    id: &str,
    commit: &str,
) -> anyhow::Result<(PathBuf, Vec<String>)> {
    if !is_valid_apply_id(id) {
        bail!("invalid apply id");
    }
    if !is_commit_hash(commit) {
        bail!("invalid commit");
    }
    let executable = exe_path(config)?;
    let wrapper = [
        "apply".to_string(),
        "--config".into(),
        config_path.to_string_lossy().into_owned(),
        "--".into(),
        id.into(),
        commit.into(),
    ];
    Ok(match config.launcher {
        Launcher::SystemdRun => {
            let mut args = vec![
                "--unit".to_string(),
                format!("lodestar-apply-{}", &id[..id.len().min(8)]),
                "--collect".into(),
                "--property=Type=exec".into(),
                format!("--setenv=PATH={DAEMON_PATH}"),
                format!("--setenv=HOME={}", daemon_home(config)),
                executable.to_string_lossy().into_owned(),
            ];
            args.extend(wrapper);
            (PathBuf::from("systemd-run"), args)
        }
        Launcher::Detach | Launcher::Inline => (executable, wrapper.to_vec()),
    })
}

pub fn launch(
    config: &AgentConfig,
    config_path: &Path,
    id: &str,
    commit: &str,
) -> anyhow::Result<()> {
    let (program, args) = launch_command(config, config_path, id, commit)?;
    let mut command = Command::new(program);
    command.args(args);
    match config.launcher {
        Launcher::SystemdRun => {
            let status = command
                .stdin(Stdio::null())
                .status()
                .context("running systemd-run")?;
            if !status.success() {
                bail!("systemd-run failed ({status})");
            }
        }
        Launcher::Detach => {
            command
                .env("PATH", DAEMON_PATH)
                .env("HOME", daemon_home(config))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            // SAFETY: setsid is async-signal-safe and the hook does nothing else.
            unsafe {
                command.pre_exec(|| {
                    if libc::setsid() == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            reap(command.spawn().context("spawning apply wrapper")?);
        }
        Launcher::Inline => {
            command
                .env("PATH", DAEMON_PATH)
                .env("HOME", daemon_home(config));
            reap(command.spawn().context("spawning apply wrapper")?);
        }
    }
    Ok(())
}

/// Waits for the child in the background so it never lingers as a zombie.
fn reap(mut child: std::process::Child) {
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}
