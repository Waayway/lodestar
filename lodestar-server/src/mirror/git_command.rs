//! Running git against the mirror, and the channel-name check.

use std::ffi::OsStr;
use std::process::Stdio;
use std::time::Duration;

use anyhow::Context;
use tokio::process::Command;

use super::Mirror;

/// Upper bound for network git operations (fetch, push).
const NETWORK_TIMEOUT: Duration = Duration::from_secs(120);

/// What a git run produced.
pub(super) struct GitOutput {
    pub(super) code: Option<i32>,
    pub(super) stdout: String,
    pub(super) stderr: String,
}

impl GitOutput {
    pub(super) fn ok(&self) -> bool {
        self.code == Some(0)
    }
}

pub(super) fn valid_channel(channel: &str) -> bool {
    !channel.is_empty()
        && !channel.starts_with(['-', '.', '/'])
        && !channel.contains("..")
        && channel.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | '/')
        })
}

impl Mirror {
    pub(super) async fn git<I, S>(&self, args: I) -> anyhow::Result<GitOutput>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = Command::new("git");
        command
            .arg("--git-dir")
            .arg(&self.path)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null())
            .kill_on_drop(true);
        if let Some(key) = &self.deploy_key {
            command.env(
                "GIT_SSH_COMMAND",
                format!(
                    "ssh -i {} -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new -o BatchMode=yes",
                    key.display()
                ),
            );
        }
        let output = tokio::time::timeout(NETWORK_TIMEOUT, command.output())
            .await
            .context("git timed out")?
            .context("running git")?;
        Ok(GitOutput {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}
