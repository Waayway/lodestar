//! The demo's throwaway config repo: a bare `infra.git` (the server's "GitHub") and a work
//! clone that commits to `unstable`.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, bail};
use tokio::process::Command;

use super::now;

/// Subjects of the seeded `unstable` history, oldest first.
const SEED_SUBJECTS: [&str; 12] = [
    "feat: initial flake with a shared base module",
    "feat(laptop-1): add the host",
    "feat(laptop-2): add the host",
    "feat(desktop): GPU drivers and a gaming profile",
    "fix(desktop): suspend no longer wakes immediately",
    "feat(server-1): add the host with sshd and restic",
    "feat(media): jellyfin and the media disks",
    "fix: every host evaluates again after the module split",
    "feat(vpn): wireguard server",
    "chore: nix flake update",
    "feat(workstation): add the host",
    "fix(laptop-1): fingerprint reader for sudo",
];

/// `stable` starts at this seeded commit (1-based).
pub const STABLE_AT: usize = 8;

/// Subjects for the commits that keep landing on `unstable`, used in turn.
const NEW_SUBJECTS: [&str; 8] = [
    "fix(shell): faster prompt in large git repos",
    "feat(desktop): bump the photo library service",
    "fix(media): hardware transcoding on the iGPU",
    "chore: nix flake update",
    "feat(vpn): wireguard peer for the travel router",
    "fix(laptop-1): window gaps on the external display",
    "feat(server-1): nightly backup of /var/lib",
    "fix(laptop-2): battery-friendly tailscale settings",
];

async fn git(dir: &Path, args: &[&str], date: Option<i64>) -> anyhow::Result<String> {
    let mut cmd = Command::new("git");
    cmd.current_dir(dir)
        .args([
            "-c",
            "user.name=lodestar-demo",
            "-c",
            "user.email=demo@lodestar.invalid",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .kill_on_drop(true);
    if let Some(t) = date {
        let d = format!("@{t} +0000");
        cmd.env("GIT_AUTHOR_DATE", &d).env("GIT_COMMITTER_DATE", &d);
    }
    let out = cmd
        .output()
        .await
        .with_context(|| format!("running git {args:?}"))?;
    if !out.status.success() {
        bail!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub struct DemoRepo {
    /// The bare repo: the server's `repo_url` and `push_url`.
    pub bare: PathBuf,
    work: PathBuf,
    next_subject: usize,
}

impl DemoRepo {
    /// Creates the repos under `dir` and seeds `unstable` (12 commits, a few hours apart)
    /// and `stable` (at the 8th). Returns the repo and the seeded commits, oldest first.
    pub async fn create(dir: &Path) -> anyhow::Result<(DemoRepo, Vec<String>)> {
        let bare = dir.join("infra.git");
        let work = dir.join("work");
        std::fs::create_dir_all(&work).with_context(|| format!("creating {}", work.display()))?;
        let bare_s = bare.to_str().context("temp dir path is not UTF-8")?;
        git(
            dir,
            &["init", "--quiet", "--bare", "-b", "unstable", bare_s],
            None,
        )
        .await?;
        git(&work, &["init", "--quiet", "-b", "unstable"], None).await?;
        git(&work, &["remote", "add", "origin", bare_s], None).await?;
        let repo = DemoRepo {
            bare,
            work,
            next_subject: 0,
        };
        let start = now() - 3 * 24 * 3600;
        let mut commits = Vec::with_capacity(SEED_SUBJECTS.len());
        for (i, subject) in SEED_SUBJECTS.iter().enumerate() {
            commits.push(repo.commit_at(subject, start + i as i64 * 5 * 3600).await?);
        }
        git(
            &repo.work,
            &["push", "--quiet", "origin", "HEAD:refs/heads/unstable"],
            None,
        )
        .await?;
        let stable = format!("{}:refs/heads/stable", commits[STABLE_AT - 1]);
        git(&repo.work, &["push", "--quiet", "origin", &stable], None).await?;
        Ok((repo, commits))
    }

    async fn commit_at(&self, subject: &str, time: i64) -> anyhow::Result<String> {
        git(
            &self.work,
            &["commit", "--quiet", "--allow-empty", "-m", subject],
            Some(time),
        )
        .await?;
        git(&self.work, &["rev-parse", "HEAD"], None).await
    }

    /// Commits the next realistic subject to `unstable` and pushes it.
    pub async fn land_commit(&mut self) -> anyhow::Result<String> {
        let subject = NEW_SUBJECTS[self.next_subject % NEW_SUBJECTS.len()];
        self.next_subject += 1;
        let commit = self.commit_at(subject, now()).await?;
        git(
            &self.work,
            &["push", "--quiet", "origin", "HEAD:refs/heads/unstable"],
            None,
        )
        .await?;
        Ok(commit)
    }
}
