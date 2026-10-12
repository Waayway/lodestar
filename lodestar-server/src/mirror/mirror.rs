use std::path::PathBuf;

use anyhow::{Context, bail};
use lodestar_proto::{PromoteChannels, is_commit_hash};

use super::PromoteError;
use super::git_command::valid_channel;
use crate::config::ServerConfig;

pub struct Mirror {
    pub(super) path: PathBuf,
    repo_url: String,
    push_url: String,
    pub(super) deploy_key: Option<PathBuf>,
    /// The promote pair; `None` disables promote.
    promote: Option<PromoteChannels>,
    /// Serialises ref writers (fetch, promote): concurrent `git fetch`es on one bare repo
    /// fail with "cannot lock ref".
    write_lock: tokio::sync::Mutex<()>,
}

impl Mirror {
    /// Creates `<state_dir>/mirror.git` when missing and does a first fetch. A failing first
    /// fetch is not fatal (the server may start offline); later `fetch` calls report errors.
    pub async fn open_or_init(config: &ServerConfig) -> anyhow::Result<Mirror> {
        let path = config.state_dir.join("mirror.git");
        let mirror = Mirror {
            path,
            repo_url: config.repo_url.clone(),
            push_url: config.push_url.clone(),
            deploy_key: config.deploy_key_file.clone(),
            promote: config.promote.clone(),
            write_lock: tokio::sync::Mutex::new(()),
        };
        if !mirror.path.join("HEAD").exists() {
            std::fs::create_dir_all(&mirror.path)
                .with_context(|| format!("creating {}", mirror.path.display()))?;
            let output = mirror.git(["init", "--bare", "--quiet"]).await?;
            if !output.ok() {
                bail!("git init failed: {}", output.stderr.trim());
            }
        }
        if let Err(error) = mirror.fetch().await {
            eprintln!("lodestar-server: initial mirror fetch failed: {error:#}");
        }
        Ok(mirror)
    }

    pub async fn fetch(&self) -> anyhow::Result<()> {
        let _guard = self.write_lock.lock().await;
        let output = self
            .git([
                "fetch",
                "--quiet",
                "--prune",
                "--",
                &self.repo_url,
                "+refs/heads/*:refs/heads/*",
            ])
            .await?;
        if !output.ok() {
            bail!("git fetch failed: {}", output.stderr.trim());
        }
        Ok(())
    }

    /// Commit at the head of `channel`, if the branch exists in the mirror (git errors read as
    /// absent; see `try_head`).
    pub async fn head(&self, channel: &str) -> Option<String> {
        self.try_head(channel).await.ok().flatten()
    }

    /// Commit at the head of `channel`: `Ok(None)` when the branch does not exist (or the name
    /// is not a valid channel), `Err` when git itself failed.
    pub async fn try_head(&self, channel: &str) -> anyhow::Result<Option<String>> {
        if !valid_channel(channel) {
            return Ok(None);
        }
        let refname = format!("refs/heads/{channel}^{{commit}}");
        let output = self
            .git(["rev-parse", "--verify", "--quiet", &refname])
            .await?;
        let hash = output.stdout.trim();
        match output.code {
            Some(0) if is_commit_hash(hash) => Ok(Some(hash.to_string())),
            // --verify --quiet: exit 1 and no output for a missing ref.
            Some(1) if hash.is_empty() => Ok(None),
            _ => bail!(
                "git rev-parse {refname} failed ({:?}): {}",
                output.code,
                output.stderr.trim()
            ),
        }
    }

    /// First line of the commit message.
    pub async fn subject(&self, commit: &str) -> Option<String> {
        if !is_commit_hash(commit) {
            return None;
        }
        let output = self
            .git(["log", "-1", "--format=%s", commit, "--"])
            .await
            .ok()?;
        output.ok().then(|| output.stdout.trim().to_string())
    }

    async fn has_commit(&self, commit: &str) -> bool {
        is_commit_hash(commit)
            && self
                .git(["cat-file", "-e", &format!("{commit}^{{commit}}")])
                .await
                .is_ok_and(|output| output.ok())
    }

    /// `Ok(true)` when `ancestor` is an ancestor of (or equal to) `descendant`.
    /// Both must be full hashes or full ref names.
    async fn is_ancestor(&self, ancestor: &str, descendant: &str) -> anyhow::Result<bool> {
        let output = self
            .git(["merge-base", "--is-ancestor", ancestor, descendant])
            .await?;
        match output.code {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => bail!("git merge-base failed: {}", output.stderr.trim()),
        }
    }

    /// Commits on `channel` that are not in `running` (a trailing `-dirty` is ignored).
    /// `None` when `running` is not a commit hash, is not in the mirror, or the channel is unknown.
    pub async fn behind_count(&self, running: &str, channel: &str) -> Option<u32> {
        let running = running.strip_suffix("-dirty").unwrap_or(running);
        if !is_commit_hash(running) || !valid_channel(channel) || !self.has_commit(running).await {
            return None;
        }
        let range = format!("{running}..refs/heads/{channel}");
        let output = self.git(["rev-list", "--count", &range, "--"]).await.ok()?;
        if !output.ok() {
            return None;
        }
        output.stdout.trim().parse().ok()
    }

    /// The promote pair this mirror was configured with (`None`: promote is disabled).
    pub fn promote_channels(&self) -> Option<&PromoteChannels> {
        self.promote.as_ref()
    }

    /// The `from` head when the `to` head is a strict ancestor of it.
    pub async fn promote_candidate(&self) -> Option<String> {
        let channels = self.promote.as_ref()?;
        let from = self.head(&channels.from).await?;
        let to = self.head(&channels.to).await?;
        if to == from {
            return None;
        }
        self.is_ancestor(&to, &from)
            .await
            .ok()
            .filter(|&is_ancestor| is_ancestor)
            .map(|_| from)
    }

    /// Fast-forwards `to` to `commit` (non-force push). `commit` must be on `from` and `to`
    /// must be an ancestor of it; otherwise `PromoteError::Rejected`.
    pub async fn promote(&self, commit: &str) -> Result<(), PromoteError> {
        let _guard = self.write_lock.lock().await;
        let reject = |message: String| Err(PromoteError::Rejected(message));
        let Some(channels) = &self.promote else {
            return reject("promote is disabled on this server".into());
        };
        if !valid_channel(&channels.from) || !valid_channel(&channels.to) {
            return reject(format!(
                "invalid promote channels {} -> {}",
                channels.from, channels.to
            ));
        }
        if !is_commit_hash(commit) {
            return reject("not a commit hash".into());
        }
        if !self.has_commit(commit).await {
            return reject(format!("commit {commit} is not in the mirror"));
        }
        if !self
            .is_ancestor(commit, &format!("refs/heads/{}", channels.from))
            .await?
        {
            return reject(format!("commit {commit} is not on {}", channels.from));
        }
        // A git error here must not read as "no target yet" (which would skip the check).
        if let Some(to) = self.try_head(&channels.to).await?
            && !self.is_ancestor(&to, commit).await?
        {
            return reject(format!(
                "{} ({to}) is not an ancestor of {commit}",
                channels.to
            ));
        }
        let output = self
            .git([
                "push",
                "--quiet",
                "--",
                &self.push_url,
                &format!("{commit}:refs/heads/{}", channels.to),
            ])
            .await?;
        if !output.ok() {
            return Err(anyhow::anyhow!("git push failed: {}", output.stderr.trim()).into());
        }
        Ok(())
    }
}
