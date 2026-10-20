//! A bare git "origin" with a working clone, and a `git` runner with neutral config.

use std::path::{Path, PathBuf};
use std::process::Command;

pub fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

/// A bare "origin" plus a working clone used to create commits on it.
pub struct Origin {
    pub bare: PathBuf,
    pub work: PathBuf,
}

impl Origin {
    pub fn new(dir: &Path) -> Origin {
        let bare = dir.join("origin.git");
        let work = dir.join("work");
        std::fs::create_dir_all(&work).unwrap();
        git(
            dir,
            &["init", "--bare", "-b", "main", bare.to_str().unwrap()],
        );
        git(&work, &["init", "-b", "unstable"]);
        git(&work, &["remote", "add", "origin", bare.to_str().unwrap()]);
        Origin { bare, work }
    }

    /// Commit on the current branch and push it; returns the new hash.
    pub fn commit(&self, subject: &str, push_branch: &str) -> String {
        git(&self.work, &["commit", "--allow-empty", "-m", subject]);
        let commit = git(&self.work, &["rev-parse", "HEAD"]);
        self.push(push_branch);
        commit
    }

    pub fn push(&self, branch: &str) {
        git(
            &self.work,
            &["push", "-f", "origin", &format!("HEAD:refs/heads/{branch}")],
        );
    }

    pub fn bare_ref(&self, branch: &str) -> String {
        git(&self.bare, &["rev-parse", &format!("refs/heads/{branch}")])
    }
}
