use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use nix::unistd::{Uid, User};

use crate::config::AgentConfig;
use crate::privileges::drop_privileges;

/// Runs git against the user's flake checkout, as that user.
pub struct Git {
    flake_path: PathBuf,
    user: String,
    /// (uid, gid, home); set only when running as root for a non-root user.
    run_as: Option<(u32, u32, PathBuf)>,
}

impl Git {
    pub fn new(config: &AgentConfig) -> anyhow::Result<Git> {
        let run_as = if Uid::effective().is_root() && config.user != "root" {
            let user = User::from_name(&config.user)
                .with_context(|| format!("looking up user {}", config.user))?
                .with_context(|| format!("no such user: {}", config.user))?;
            Some((user.uid.as_raw(), user.gid.as_raw(), user.dir))
        } else {
            None
        };
        Ok(Git {
            flake_path: config.flake_path.clone(),
            user: config.user.clone(),
            run_as,
        })
    }

    /// A command for `program` that runs as the configured user (privileges
    /// are dropped when the agent runs as root; otherwise it runs as-is).
    pub fn user_command(&self, program: &str) -> anyhow::Result<Command> {
        let mut command = Command::new(program);
        if let Some((uid, gid, home)) = &self.run_as {
            drop_privileges(&mut command, &self.user, *uid, *gid, home)
                .context("preparing privilege drop")?;
        }
        Ok(command)
    }

    /// Runs `git -C <flake_path> <args>`. Non-zero exit is not an error here.
    pub fn run(&self, args: &[&str]) -> anyhow::Result<Output> {
        self.git_command(args)?
            .output()
            .with_context(|| format!("running git {}", args.join(" ")))
    }

    /// `git -C <flake_path> <args>` as the configured user, never prompting.
    fn git_command(&self, args: &[&str]) -> anyhow::Result<Command> {
        let mut command = self.user_command("git")?;
        command
            .arg("-C")
            .arg(&self.flake_path)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("LC_ALL", "C");
        Ok(command)
    }

    fn succeeds(&self, args: &[&str]) -> bool {
        self.run(args)
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn stdout(&self, args: &[&str]) -> Option<String> {
        let output = self.run(args).ok()?;
        if !output.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    pub fn fetch(&self, repo_url: &str) -> anyhow::Result<()> {
        let output = self.run(&Self::fetch_args(repo_url))?;
        if !output.status.success() {
            bail!(
                "git fetch failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }

    /// `fetch`, but killed (with everything it started) after `timeout`.
    pub fn fetch_within(&self, repo_url: &str, timeout: Duration) -> anyhow::Result<()> {
        let mut command = self.git_command(&Self::fetch_args(repo_url))?;
        // Its own process group, so a kill also reaches git-remote-* and ssh.
        command
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = command.spawn().context("running git fetch")?;
        let mut stderr = child.stderr.take().context("git fetch stderr")?;
        let reader = std::thread::spawn(move || {
            let mut buffer = Vec::new();
            let _ = stderr.read_to_end(&mut buffer);
            buffer
        });
        let deadline = Instant::now() + timeout;
        let status = loop {
            if let Some(status) = child.try_wait().context("waiting for git fetch")? {
                break status;
            }
            if Instant::now() >= deadline {
                // SAFETY: signals the process group the child leads.
                unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
                let _ = child.kill();
                let _ = child.wait();
                bail!("git fetch timed out after {} s", timeout.as_secs());
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        let error_output = reader.join().unwrap_or_default();
        if !status.success() {
            bail!(
                "git fetch failed: {}",
                String::from_utf8_lossy(&error_output).trim()
            );
        }
        Ok(())
    }

    fn fetch_args(repo_url: &str) -> [&str; 4] {
        [
            "fetch",
            "--quiet",
            repo_url,
            "+refs/heads/*:refs/remotes/origin/*",
        ]
    }

    /// None on detached HEAD.
    pub fn current_branch(&self) -> Option<String> {
        self.stdout(&["symbolic-ref", "--quiet", "--short", "HEAD"])
            .filter(|branch| !branch.is_empty())
    }

    pub fn head(&self) -> Option<String> {
        self.stdout(&["rev-parse", "HEAD"])
            .filter(|head| !head.is_empty())
    }

    /// Tracked changes only; untracked files do not count.
    pub fn is_dirty(&self) -> bool {
        !self.succeeds(&["diff", "--quiet", "HEAD"])
    }

    pub fn untracked_count(&self) -> u32 {
        self.stdout(&["ls-files", "--others", "--exclude-standard"])
            .map(|listing| listing.lines().filter(|line| !line.is_empty()).count() as u32)
            .unwrap_or(0)
    }

    pub fn commit_exists(&self, commit: &str) -> bool {
        self.succeeds(&["cat-file", "-e", &format!("{commit}^{{commit}}")])
    }

    pub fn is_ancestor(&self, ancestor: &str, descendant: &str) -> bool {
        self.succeeds(&["merge-base", "--is-ancestor", ancestor, descendant])
    }

    /// Number of local commits not in `remote_ref`; None if the ref is unknown.
    pub fn ahead_of(&self, remote_ref: &str) -> Option<u32> {
        self.stdout(&["rev-list", "--count", &format!("{remote_ref}..HEAD")])?
            .parse()
            .ok()
    }

    pub fn ff_merge(&self, commit: &str) -> anyhow::Result<()> {
        let output = self.run(&["merge", "--ff-only", commit])?;
        if !output.status.success() {
            bail!(
                "git merge --ff-only {commit} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }
}
