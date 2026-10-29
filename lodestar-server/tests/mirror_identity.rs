//! The lock commit's identity cannot be changed by the service environment. A test binary of
//! its own: it sets process-wide git identity variables.
mod support;

use std::path::PathBuf;

use lodestar_server::config::{AccessConfig, OnlineSource, ServerConfig};
use lodestar_server::mirror::Mirror;
use support::{Origin, git};

#[tokio::test]
async fn lock_commit_ignores_identity_from_the_environment() {
    // SAFETY: the only test in this binary, set before any other thread reads the environment.
    unsafe {
        for name in ["GIT_AUTHOR_NAME", "GIT_COMMITTER_NAME"] {
            std::env::set_var(name, "mallory");
        }
        for name in ["GIT_AUTHOR_EMAIL", "GIT_COMMITTER_EMAIL", "EMAIL"] {
            std::env::set_var(name, "mallory@example.com");
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    std::fs::write(origin.work.join("flake.lock"), "{}\n").unwrap();
    git(&origin.work, &["add", "flake.lock"]);
    let base = origin.commit("lock", "unstable");
    let state_dir = dir.path().join("state");
    let config = ServerConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        agent_port: 7461,
        hosts_file: state_dir.join("hosts.json"),
        links_file: None,
        repo_url: origin.bare.to_str().unwrap().into(),
        push_url: origin.bare.to_str().unwrap().into(),
        deploy_key_file: None,
        state_dir: state_dir.clone(),
        access: AccessConfig::Disabled,
        online: OnlineSource::Tailscale,
        mirror_fetch_secs: 300,
        poll_secs: 60,
        tailscale_bin: PathBuf::from("tailscale"),
        promote: Some(lodestar_proto::PromoteChannels::default()),
        proxy: None,
        updates: Default::default(),
        metrics: None,
        nix_bin: "nix".into(),
    };
    let mirror = Mirror::open_or_init(&config).await.unwrap();
    let worktree = dir.path().join("wt");
    mirror.worktree("unstable", &worktree).await.unwrap();
    std::fs::write(worktree.join("flake.lock"), "{\"nodes\":1}\n").unwrap();
    let commit = mirror
        .push_lock(&worktree, "unstable", &base, "chore(x): bump")
        .await
        .unwrap();
    assert_eq!(
        git(
            &origin.bare,
            &["log", "-1", "--format=%an <%ae>|%cn <%ce>", &commit]
        ),
        "Lodestar <lodestar@vanwaaij.com>|Lodestar <lodestar@vanwaaij.com>"
    );
    mirror.remove_worktree(&worktree).await;
}
