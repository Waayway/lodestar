//! Upstream release check against a fake provider (no network).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use lodestar_server::config::{
    AccessConfig, OnlineSource, ServerConfig, SoftwareConfig, UpdatesConfig, Upstream,
};
use lodestar_server::db;
use lodestar_server::mirror::Mirror;
use lodestar_server::state::AppState;
use lodestar_server::tailscale::StaticOnline;
use lodestar_server::updates::upstream::{UpstreamProvider, check_once, latest_release};

const LIST: &str = r#"[
  {"tag_name":"v2.0.0","draft":true,"prerelease":false},
  {"tag_name":"v1.105.0-pre","draft":false,"prerelease":true},
  {"tag_name":"v1.104.2","draft":false,"prerelease":false},
  {"tag_name":"v1.104.1","draft":false,"prerelease":false}
]"#;
const ONLY_PRE: &str = r#"[
  {"tag_name":"v1.105.0-pre","draft":false,"prerelease":true},
  {"tag_name":"v1.106.0-rc1","draft":false,"prerelease":true}
]"#;

#[test]
fn latest_release_skips_drafts_and_prereleases() {
    assert_eq!(latest_release(LIST).unwrap().as_deref(), Some("1.104.2"));
}

#[test]
fn latest_release_none_when_only_prereleases() {
    assert_eq!(latest_release(ONLY_PRE).unwrap(), None);
}

#[test]
fn latest_release_rejects_invalid_json() {
    assert!(latest_release("not json").is_err());
}

type Answer = Result<String, String>;

#[derive(Clone)]
struct Fake(Arc<Mutex<Answer>>);

#[async_trait]
impl UpstreamProvider for Fake {
    async fn releases(&self, repo: &str) -> anyhow::Result<String> {
        assert_eq!(repo, "tailscale/tailscale");
        self.0.lock().unwrap().clone().map_err(anyhow::Error::msg)
    }
}

fn empty_origin(dir: &Path) -> PathBuf {
    let bare = dir.join("origin.git");
    let output = Command::new("git")
        .args(["init", "--bare", "--quiet", bare.to_str().unwrap()])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(output.status.success());
    bare
}

async fn state(fake: Fake) -> (AppState, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let origin = empty_origin(dir.path());
    let config = ServerConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        agent_port: 7461,
        hosts_file: dir.path().join("hosts.json"),
        links_file: None,
        repo_url: origin.to_str().unwrap().into(),
        push_url: origin.to_str().unwrap().into(),
        deploy_key_file: None,
        state_dir: dir.path().join("state"),
        access: AccessConfig::Disabled,
        online: OnlineSource::Static {
            addresses: HashMap::new(),
        },
        mirror_fetch_secs: 300,
        poll_secs: 60,
        tailscale_bin: PathBuf::from("tailscale"),
        promote: None,
        proxy: None,
        updates: UpdatesConfig {
            software: BTreeMap::from([(
                "tailscale".to_string(),
                SoftwareConfig {
                    input: "nixpkgs".into(),
                    upstream: Upstream::Github("tailscale/tailscale".into()),
                },
            )]),
            ..Default::default()
        },
        metrics: None,
        nix_bin: "nix".into(),
    };
    let connection = db::connect(&config.state_dir).await.unwrap();
    let mirror = Arc::new(Mirror::open_or_init(&config).await.unwrap());
    let online = Arc::new(StaticOnline::new(&HashMap::new()).unwrap());
    let mut state = AppState::new(Arc::new(config), connection, mirror, online, vec![]).unwrap();
    state.upstream = Arc::new(fake);
    (state, dir)
}

fn latest(state: &AppState) -> lodestar_server::updates::upstream::LatestVersion {
    state.latest.lock().unwrap()["tailscale"].clone()
}

#[tokio::test]
async fn failures_keep_last_known_version_and_record_the_error() {
    let answer = Arc::new(Mutex::new(Ok(LIST.to_string())));
    let (state, _dir) = state(Fake(answer.clone())).await;

    check_once(&state).await;
    let first = latest(&state);
    assert_eq!(first.version.as_deref(), Some("1.104.2"));
    assert!(first.checked_at.is_some());
    assert_eq!(first.error, None);
    assert_eq!(first.errors, 0);

    *answer.lock().unwrap() = Err("403 rate limit".into());
    check_once(&state).await;
    let failed = latest(&state);
    assert_eq!(failed.version, first.version);
    assert_eq!(failed.checked_at, first.checked_at);
    assert!(failed.error.unwrap().contains("403 rate limit"));
    assert_eq!(failed.errors, 1);

    *answer.lock().unwrap() = Ok(ONLY_PRE.to_string());
    check_once(&state).await;
    let pre = latest(&state);
    assert_eq!(pre.version, first.version);
    assert_eq!(pre.checked_at, first.checked_at);
    assert!(pre.error.is_some());
    // "No stable release" counts as a failed check.
    assert_eq!(pre.errors, 2);

    *answer.lock().unwrap() = Ok(LIST.to_string());
    check_once(&state).await;
    assert_eq!(latest(&state).error, None);
    // A counter: a success does not reset it.
    assert_eq!(latest(&state).errors, 2);
}
