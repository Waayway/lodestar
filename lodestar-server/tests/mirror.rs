use std::path::{Path, PathBuf};
use std::process::Command;

use lodestar_server::config::{AccessConfig, OnlineSource, ServerConfig};
use lodestar_server::mirror::Mirror;

fn git(dir: &Path, args: &[&str]) -> String {
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
struct Origin {
    _dir: tempfile::TempDir,
    bare: PathBuf,
    work: PathBuf,
}

impl Origin {
    fn new() -> Origin {
        let dir = tempfile::tempdir().unwrap();
        let bare = dir.path().join("origin.git");
        let work = dir.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        git(
            dir.path(),
            &["init", "--bare", "-b", "main", bare.to_str().unwrap()],
        );
        git(&work, &["init", "-b", "unstable"]);
        git(&work, &["remote", "add", "origin", bare.to_str().unwrap()]);
        Origin {
            _dir: dir,
            bare,
            work,
        }
    }

    /// Commit on the current branch and push it; returns the new hash.
    fn commit(&self, subject: &str, push_branch: &str) -> String {
        git(&self.work, &["commit", "--allow-empty", "-m", subject]);
        let commit = git(&self.work, &["rev-parse", "HEAD"]);
        git(
            &self.work,
            &["push", "origin", &format!("HEAD:refs/heads/{push_branch}")],
        );
        commit
    }

    fn bare_ref(&self, branch: &str) -> String {
        git(&self.bare, &["rev-parse", &format!("refs/heads/{branch}")])
    }
}

fn config(state_dir: &Path, repository_url: &str) -> ServerConfig {
    ServerConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        agent_port: 7461,
        hosts_file: state_dir.join("hosts.json"),
        links_file: None,
        repo_url: repository_url.into(),
        push_url: repository_url.into(),
        deploy_key_file: None,
        state_dir: state_dir.to_path_buf(),
        access: AccessConfig::Disabled,
        online: OnlineSource::Tailscale,
        mirror_fetch_secs: 300,
        poll_secs: 60,
        tailscale_bin: PathBuf::from("tailscale"),
        promote: Some(lodestar_proto::PromoteChannels::default()),
        proxy: None,
    }
}

async fn mirror_of(origin: &Origin) -> (Mirror, tempfile::TempDir) {
    let state = tempfile::tempdir().unwrap();
    let mirror = Mirror::open_or_init(&config(state.path(), origin.bare.to_str().unwrap()))
        .await
        .unwrap();
    (mirror, state)
}

#[tokio::test]
async fn head_after_fetch() {
    let origin = Origin::new();
    let first_commit = origin.commit("first", "unstable");
    let (mirror, _state_dir) = mirror_of(&origin).await;
    assert_eq!(mirror.head("unstable").await, Some(first_commit.clone()));
    assert_eq!(mirror.head("stable").await, None);
    assert_eq!(
        mirror.subject(&first_commit).await.as_deref(),
        Some("first")
    );
    assert_eq!(mirror.subject("--output=x").await, None);
    let second_commit = origin.commit("second", "unstable");
    mirror.fetch().await.unwrap();
    assert_eq!(mirror.head("unstable").await, Some(second_commit));
}

#[tokio::test]
async fn try_head_tells_absent_from_git_error() {
    let origin = Origin::new();
    let first_commit = origin.commit("first", "unstable");
    let (mirror, state_dir) = mirror_of(&origin).await;
    assert_eq!(
        mirror.try_head("unstable").await.unwrap(),
        Some(first_commit)
    );
    assert_eq!(mirror.try_head("stable").await.unwrap(), None);
    // A broken mirror: git fails, which is not the same as "no stable branch".
    std::fs::remove_file(state_dir.path().join("mirror.git/HEAD")).unwrap();
    assert!(mirror.try_head("stable").await.is_err());
    assert_eq!(mirror.head("stable").await, None);
}

#[tokio::test]
async fn behind_strips_dirty() {
    let origin = Origin::new();
    let first_commit = origin.commit("one", "unstable");
    origin.commit("two", "unstable");
    origin.commit("three", "unstable");
    let (mirror, _state_dir) = mirror_of(&origin).await;
    assert_eq!(
        mirror
            .behind_count(&format!("{first_commit}-dirty"), "unstable")
            .await,
        Some(2)
    );
    assert_eq!(
        mirror.behind_count(&first_commit, "unstable").await,
        Some(2)
    );
}

#[tokio::test]
async fn behind_unknown_for_foreign_or_unknown() {
    let origin = Origin::new();
    origin.commit("one", "unstable");
    let (mirror, _state_dir) = mirror_of(&origin).await;
    assert_eq!(mirror.behind_count("unknown", "unstable").await, None);
    assert_eq!(mirror.behind_count(&"a".repeat(40), "unstable").await, None);
    assert_eq!(mirror.behind_count("--help", "unstable").await, None);
}

#[tokio::test]
async fn promote_rejects_non_ancestor() {
    let origin = Origin::new();
    let first_commit = origin.commit("one", "unstable");
    origin.commit("two", "stable");
    // A commit on a divergent line: on a side branch, not an ancestor of unstable.
    git(&origin.work, &["checkout", "-b", "side", &first_commit]);
    let side = origin.commit("side", "side");
    git(&origin.work, &["checkout", "unstable"]);
    let (mirror, _state_dir) = mirror_of(&origin).await;
    assert!(mirror.promote(&side).await.is_err());
    assert!(mirror.promote(&"b".repeat(40)).await.is_err());
    assert!(mirror.promote("--delete").await.is_err());
    // stable is not an ancestor of c1 (stable is ahead), so c1 is also rejected.
    assert!(mirror.promote(&first_commit).await.is_err());
}

#[tokio::test]
async fn promote_fast_forwards_stable() {
    let origin = Origin::new();
    let first_commit = origin.commit("one", "stable");
    git(
        &origin.work,
        &["push", "origin", "HEAD:refs/heads/unstable"],
    );
    let second_commit = origin.commit("two", "unstable");
    let third_commit = origin.commit("three", "unstable");
    let (mirror, _state_dir) = mirror_of(&origin).await;
    assert_eq!(mirror.promote_candidate().await, Some(third_commit.clone()));
    mirror.promote(&second_commit).await.unwrap();
    assert_eq!(origin.bare_ref("stable"), second_commit);
    assert_ne!(first_commit, second_commit);
    mirror.fetch().await.unwrap();
    assert_eq!(mirror.head("stable").await, Some(second_commit));
    assert_eq!(mirror.promote_candidate().await, Some(third_commit));
}

#[tokio::test]
async fn promote_candidate_none_when_equal() {
    let origin = Origin::new();
    origin.commit("one", "stable");
    origin.commit("two", "unstable");
    git(&origin.work, &["push", "origin", "HEAD:refs/heads/stable"]);
    let (mirror, _state_dir) = mirror_of(&origin).await;
    assert_eq!(mirror.promote_candidate().await, None);
}

#[tokio::test]
async fn fetch_error_is_err() {
    let state = tempfile::tempdir().unwrap();
    let bogus = state.path().join("does-not-exist.git");
    let server_config = config(state.path(), bogus.to_str().unwrap());
    // open_or_init tolerates a failing first fetch; fetch() reports it.
    let mirror = Mirror::open_or_init(&server_config).await.unwrap();
    assert!(mirror.fetch().await.is_err());
    assert_eq!(mirror.head("unstable").await, None);
}

#[tokio::test]
async fn promote_uses_the_configured_channels() {
    let origin = Origin::new();
    origin.commit("one", "release");
    git(&origin.work, &["push", "origin", "HEAD:refs/heads/main"]);
    let second_commit = origin.commit("two", "main");
    let state = tempfile::tempdir().unwrap();
    let mut server_config = config(state.path(), origin.bare.to_str().unwrap());
    server_config.promote = Some(lodestar_proto::PromoteChannels {
        from: "main".into(),
        to: "release".into(),
    });
    let mirror = Mirror::open_or_init(&server_config).await.unwrap();
    assert_eq!(
        mirror.promote_candidate().await,
        Some(second_commit.clone())
    );
    mirror.promote(&second_commit).await.unwrap();
    assert_eq!(origin.bare_ref("release"), second_commit);
}

#[tokio::test]
async fn promote_disabled() {
    let origin = Origin::new();
    origin.commit("one", "stable");
    git(
        &origin.work,
        &["push", "origin", "HEAD:refs/heads/unstable"],
    );
    let second_commit = origin.commit("two", "unstable");
    let state = tempfile::tempdir().unwrap();
    let mut server_config = config(state.path(), origin.bare.to_str().unwrap());
    server_config.promote = None;
    let mirror = Mirror::open_or_init(&server_config).await.unwrap();
    assert_eq!(mirror.promote_candidate().await, None);
    assert!(mirror.promote(&second_commit).await.is_err());
    assert_ne!(origin.bare_ref("stable"), second_commit);
}
