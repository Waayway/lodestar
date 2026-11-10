//! The build gate against a fake builder and a bare-repo origin (no network, no real Nix).

mod support;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use lodestar_server::builds::{
    BuildBackoff, BuildGate, Builder, HostBuild, HostResult, build_channel, prebuild_new_heads,
    spawn_prebuild,
};
use lodestar_server::config::{AccessConfig, ManagedHost, OnlineSource, ServerConfig};
use lodestar_server::db;
use lodestar_server::mirror::Mirror;
use lodestar_server::state::AppState;
use lodestar_server::tailscale::StaticOnline;
use support::{FakeBuilder as CommitBuilder, Origin, git, path_of};
use tokio::sync::Semaphore;

const DAY: i64 = 24 * 60 * 60;

/// A builder that counts calls, can block until released, panic, fail hosts or fail publish.
#[derive(Default)]
struct FakeBuilder {
    builds: AtomicUsize,
    /// Host names of every `build` call, in order.
    built_hosts: Mutex<Vec<Vec<String>>>,
    /// Whether the worktree had the commit's `flake.lock` during each build.
    saw_worktree: Mutex<Vec<bool>>,
    /// When set, `build` waits for a permit (tests add permits to release it).
    gate: Option<Arc<Semaphore>>,
    /// Signalled when `build` is entered.
    entered: tokio::sync::Notify,
    panics: bool,
    /// Every host fails with this reason.
    fail_all: Option<String>,
    publish_error: Option<String>,
    /// Every publish: (commit, paths, channels).
    published: Mutex<Vec<(String, Vec<String>, serde_json::Value)>>,
}

impl FakeBuilder {
    fn count(&self) -> usize {
        self.builds.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Builder for FakeBuilder {
    async fn build(&self, dir: &Path, hosts: &[ManagedHost]) -> BTreeMap<String, HostBuild> {
        self.builds.fetch_add(1, Ordering::SeqCst);
        self.built_hosts
            .lock()
            .unwrap()
            .push(hosts.iter().map(|host| host.name.clone()).collect());
        self.saw_worktree
            .lock()
            .unwrap()
            .push(dir.join("flake.lock").exists());
        self.entered.notify_one();
        if let Some(gate) = &self.gate {
            gate.acquire().await.unwrap().forget();
        }
        if self.panics {
            panic!("fake builder panicked");
        }
        hosts
            .iter()
            .map(|host| {
                let build = match &self.fail_all {
                    Some(reason) => HostBuild {
                        ok: false,
                        path: None,
                        log_tail: Some(reason.clone()),
                    },
                    None => HostBuild {
                        ok: true,
                        path: Some(store_path(&host.name)),
                        log_tail: None,
                    },
                };
                (host.name.clone(), build)
            })
            .collect()
    }

    async fn publish(
        &self,
        commit: &str,
        paths: &[String],
        channels: &serde_json::Value,
    ) -> anyhow::Result<()> {
        self.published
            .lock()
            .unwrap()
            .push((commit.into(), paths.to_vec(), channels.clone()));
        match &self.publish_error {
            Some(message) => anyhow::bail!("{message}"),
            None => Ok(()),
        }
    }
}

fn store_path(name: &str) -> String {
    format!("/nix/store/0123456789abcdfghijklmnpqrsvwxyz-nixos-system-{name}")
}

fn host(name: &str, os: &str) -> ManagedHost {
    ManagedHost {
        name: name.into(),
        system: if os == "darwin" {
            "aarch64-darwin"
        } else {
            "x86_64-linux"
        }
        .into(),
        user: "alice".into(),
        channel: "unstable".into(),
        os: os.into(),
    }
}

struct Fixture {
    state: AppState,
    origin: Origin,
    commit: String,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn builds_dir(&self) -> PathBuf {
        self.state.config.state_dir.join("builds")
    }

    fn install(&mut self, builder: FakeBuilder) -> Arc<FakeBuilder> {
        let builder = Arc::new(builder);
        self.state.builder = builder.clone();
        builder
    }
}

async fn fixture() -> Fixture {
    fixture_with_hosts(vec![
        host("a", "linux"),
        host("b", "linux"),
        host("c", "darwin"),
    ])
    .await
}

/// `host` on `channel`.
fn on(channel: &str, host: ManagedHost) -> ManagedHost {
    ManagedHost {
        channel: channel.into(),
        ..host
    }
}

async fn fixture_with_hosts(hosts: Vec<ManagedHost>) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    std::fs::write(origin.work.join("flake.lock"), "{}\n").unwrap();
    git(&origin.work, &["add", "flake.lock"]);
    let commit = origin.commit("lock", "unstable");
    let config = ServerConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        agent_port: 7461,
        hosts_file: dir.path().join("hosts.json"),
        links_file: None,
        repo_url: origin.bare.to_str().unwrap().into(),
        push_url: origin.bare.to_str().unwrap().into(),
        deploy_key_file: None,
        state_dir: dir.path().join("state"),
        access: AccessConfig::Disabled,
        allowed_hosts: Vec::new(),
        online: OnlineSource::Static {
            addresses: HashMap::new(),
        },
        mirror_fetch_secs: 300,
        poll_secs: 60,
        tailscale_bin: PathBuf::from("tailscale"),
        promote: None,
        proxy: None,
        updates: Default::default(),
        builds: lodestar_server::config::BuildsConfig {
            enable: true,
            ..Default::default()
        },
        metrics: None,
        nix_bin: "nix".into(),
    };
    let connection = db::connect(&config.state_dir).await.unwrap();
    let mirror = Arc::new(Mirror::open_or_init(&config).await.unwrap());
    let online = Arc::new(StaticOnline::new(&HashMap::new()).unwrap());
    let state = AppState::new(Arc::new(config), connection, mirror, online, hosts).unwrap();
    Fixture {
        state,
        origin,
        commit,
        _dir: dir,
    }
}

fn linux_hosts() -> Vec<ManagedHost> {
    vec![host("a", "linux"), host("b", "linux")]
}

fn ok(name: &str) -> HostResult {
    HostResult {
        ok: true,
        path: Some(store_path(name)),
        log_tail: None,
        built_at: 1,
    }
}

/// Takes `path`'s lock on a separate open file (as another process would).
fn lock_file(path: &Path) -> std::fs::File {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .unwrap();
    file.lock().unwrap();
    file
}

/// Whether `path`'s lock can be taken within two seconds. Another test's child process may hold
/// a copy of the descriptor for a moment between its fork and exec, so one attempt can fail.
fn lock_is_free(path: &Path) -> bool {
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while file.try_lock().is_err() {
        if std::time::Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    true
}

fn set_age(path: &Path, now: i64, days: i64) {
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    let time = SystemTime::UNIX_EPOCH + Duration::from_secs((now - days * DAY) as u64);
    file.set_modified(time).unwrap();
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

#[tokio::test(flavor = "multi_thread")]
async fn second_caller_waits_and_builds_nothing() {
    let mut fixture = fixture().await;
    let gate = Arc::new(Semaphore::new(0));
    let builder = fixture.install(FakeBuilder {
        gate: Some(gate.clone()),
        ..Default::default()
    });
    let call = |state: AppState, commit: String| {
        tokio::spawn(async move {
            state
                .builds
                .ensure_built(&state, &commit, &linux_hosts())
                .await
                .unwrap()
        })
    };
    let first = call(fixture.state.clone(), fixture.commit.clone());
    // The first caller is inside the build (holding the lock) before the second starts.
    builder.entered.notified().await;
    let second = call(fixture.state.clone(), fixture.commit.clone());
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!second.is_finished());
    gate.add_permits(10);
    let first = first.await.unwrap();
    let second = second.await.unwrap();
    assert_eq!(builder.count(), 1);
    assert_eq!(builder.saw_worktree.lock().unwrap().as_slice(), &[true]);
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(&second).unwrap()
    );
    assert_eq!(
        first.hosts.keys().collect::<Vec<_>>(),
        vec!["a", "b"],
        "both hosts built"
    );
    assert!(first.hosts.values().all(|result| result.ok));
    assert!(first.failed_hosts(&linux_hosts()).is_empty());
    assert!(first.missing(&linux_hosts()).is_empty());
    // The worktree is gone.
    assert!(
        !fixture
            .builds_dir()
            .join("work")
            .join(&fixture.commit)
            .exists()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn lock_held_elsewhere_blocks() {
    let mut fixture = fixture().await;
    let builder = fixture.install(FakeBuilder::default());
    let lock_path = fixture
        .builds_dir()
        .join(format!("{}.lock", fixture.commit));
    let held = lock_file(&lock_path);
    let state = fixture.state.clone();
    let commit = fixture.commit.clone();
    let task = tokio::spawn(async move {
        state
            .builds
            .ensure_built(&state, &commit, &linux_hosts())
            .await
            .unwrap()
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(builder.count(), 0);
    assert!(!task.is_finished());
    drop(held);
    let result = tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(builder.count(), 1);
    assert_eq!(result.hosts.len(), 2);
}

#[tokio::test]
async fn restart_reuses_result() {
    let mut fixture = fixture().await;
    let builder = fixture.install(FakeBuilder::default());
    let state = &fixture.state;
    state
        .builds
        .ensure_built(state, &fixture.commit, &linux_hosts())
        .await
        .unwrap();
    assert_eq!(builder.count(), 1);
    let restarted = BuildGate::new(fixture.builds_dir());
    let result = restarted
        .ensure_built(state, &fixture.commit, &linux_hosts())
        .await
        .unwrap();
    assert_eq!(builder.count(), 1);
    assert_eq!(result.hosts.len(), 2);
    assert_eq!(
        restarted.read(&fixture.commit).unwrap().hosts["a"].path,
        Some(store_path("a"))
    );
}

#[tokio::test]
async fn only_missing_hosts_are_built() {
    let mut fixture = fixture().await;
    let builder = fixture.install(FakeBuilder::default());
    let state = &fixture.state;
    state
        .builds
        .record_built(
            state,
            &fixture.commit,
            BTreeMap::from([("a".to_string(), ok("a"))]),
        )
        .await
        .unwrap();
    let result = state
        .builds
        .ensure_built(state, &fixture.commit, &linux_hosts())
        .await
        .unwrap();
    // a is realised again with b in the same call: the channel advances to the commit and
    // publishes a's path, which the builder may have collected since it was recorded.
    assert_eq!(
        builder.built_hosts.lock().unwrap().as_slice(),
        &[vec!["a".to_string(), "b".to_string()]]
    );
    assert_eq!(result.hosts.keys().collect::<Vec<_>>(), vec!["a", "b"]);
    assert_eq!(result.hosts["a"].built_at, 1, "existing entry kept");
    // The record published its new path (the channel still missed b); the build of b advanced
    // the channel and published every host of it, with the channel index.
    let published = builder.published.lock().unwrap();
    let index = serde_json::json!({ "unstable": { "current": fixture.commit, "previous": null } });
    assert_eq!(published.len(), 2);
    assert_eq!(published[0].0, fixture.commit);
    assert_eq!(published[0].1, vec![store_path("a")]);
    assert_eq!(published[0].2, serde_json::json!({}));
    assert_eq!(published[1].1, vec![store_path("a"), store_path("b")]);
    assert_eq!(published[1].2, index);
}

#[tokio::test]
async fn recorded_commits_are_published_once() {
    let mut fixture = fixture().await;
    let builder = fixture.install(FakeBuilder::default());
    let state = &fixture.state;
    let results = state
        .builds
        .build_dir(
            state,
            &fixture.origin.work,
            &[host("a", "linux"), host("b", "linux"), host("c", "darwin")],
        )
        .await;
    assert_eq!(results.keys().collect::<Vec<_>>(), vec!["a", "b"]);
    let recorded = state
        .builds
        .record_built(state, &fixture.commit, results)
        .await
        .unwrap();
    assert!(recorded.failed_hosts(&linux_hosts()).is_empty());
    {
        let published = builder.published.lock().unwrap();
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].1, vec![store_path("a"), store_path("b")]);
        assert_eq!(
            published[0].2,
            serde_json::json!({ "unstable": { "current": fixture.commit, "previous": null } })
        );
    }
    assert_eq!(builder.count(), 1);
    let result = state
        .builds
        .ensure_built(state, &fixture.commit, &linux_hosts())
        .await
        .unwrap();
    assert_eq!(result, recorded);
    assert_eq!(builder.count(), 1, "nothing built again");
    assert_eq!(
        builder.published.lock().unwrap().len(),
        1,
        "nothing published again"
    );

    // A failing publish records nothing and is an error.
    let failing = fixture.install(FakeBuilder {
        publish_error: Some("publishing failed: disk full".into()),
        ..Default::default()
    });
    let state = &fixture.state;
    let other = "a".repeat(40);
    let error = state
        .builds
        .record_built(state, &other, BTreeMap::from([("a".to_string(), ok("a"))]))
        .await
        .unwrap_err();
    assert_eq!(failing.published.lock().unwrap().len(), 1);
    assert!(
        format!("{error:#}").starts_with("publish failed: "),
        "{error:#}"
    );
    assert!(state.builds.read(&other).is_none());
    assert!(
        state
            .builds
            .record_built(state, "--x", BTreeMap::new())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn no_partial_result_on_error() {
    let mut fixture = fixture().await;
    let lock_path = fixture
        .builds_dir()
        .join(format!("{}.lock", fixture.commit));
    let result_path = fixture
        .builds_dir()
        .join(format!("{}.json", fixture.commit));

    // A builder that panics: nothing is written, the lock is free, the worktree is gone.
    let panicking = fixture.install(FakeBuilder {
        panics: true,
        ..Default::default()
    });
    let state = &fixture.state;
    let error = state
        .builds
        .ensure_built(state, &fixture.commit, &linux_hosts())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("builder panicked"), "{error:#}");
    assert_eq!(panicking.count(), 1);
    assert!(!result_path.exists());
    assert!(lock_is_free(&lock_path));
    assert!(
        !fixture
            .builds_dir()
            .join("work")
            .join(&fixture.commit)
            .exists()
    );

    // A failing publish: nothing is recorded, the hosts stay missing and the call is an error.
    let failing = fixture.install(FakeBuilder {
        publish_error: Some("builder unreachable: ssh: connect refused".into()),
        ..Default::default()
    });
    let state = &fixture.state;
    let error = tokio::time::timeout(
        Duration::from_secs(10),
        state
            .builds
            .ensure_built(state, &fixture.commit, &linux_hosts()),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert_eq!(failing.count(), 1);
    let message = format!("{error:#}");
    assert!(
        message.starts_with("publish failed: builder unreachable"),
        "{message}"
    );
    assert!(!result_path.exists());
    assert!(lock_is_free(&lock_path));

    // Once publishing works again, the next call builds and records them.
    let back = fixture.install(FakeBuilder::default());
    let state = &fixture.state;
    let result = state
        .builds
        .ensure_built(state, &fixture.commit, &linux_hosts())
        .await
        .unwrap();
    assert_eq!(back.count(), 1);
    assert!(result.failed_hosts(&linux_hosts()).is_empty());
    assert!(result.missing(&linux_hosts()).is_empty());
}

#[tokio::test]
async fn darwin_hosts_are_never_built() {
    let mut fixture = fixture().await;
    let builder = fixture.install(FakeBuilder::default());
    let state = &fixture.state;
    let hosts = vec![host("a", "linux"), host("c", "darwin")];
    let result = state
        .builds
        .ensure_built(state, &fixture.commit, &hosts)
        .await
        .unwrap();
    assert_eq!(
        builder.built_hosts.lock().unwrap().as_slice(),
        &[vec!["a".to_string()]]
    );
    assert_eq!(result.hosts.keys().collect::<Vec<_>>(), vec!["a"]);
    assert!(result.missing(&hosts).is_empty());
    assert!(result.failed_hosts(&hosts).is_empty());

    // A darwin-only call builds nothing and never touches the lock.
    let other = fixture.origin.commit("other", "unstable");
    state.mirror.fetch().await.unwrap();
    let result = state
        .builds
        .ensure_built(state, &other, &[host("c", "darwin")])
        .await
        .unwrap();
    assert!(result.hosts.is_empty());
    assert_eq!(builder.count(), 1);
    assert!(!fixture.builds_dir().join(format!("{other}.lock")).exists());
}

#[tokio::test]
async fn builder_unreachable_is_not_recorded() {
    let mut fixture = fixture().await;
    let unreachable = fixture.install(FakeBuilder {
        fail_all: Some("builder unreachable: error: cannot connect to 'alice@example.com'".into()),
        ..Default::default()
    });
    let state = fixture.state.clone();
    let error = state
        .builds
        .ensure_built(&state, &fixture.commit, &linux_hosts())
        .await
        .unwrap_err();
    assert_eq!(
        format!("{error:#}"),
        "builder unreachable: error: cannot connect to 'alice@example.com'"
    );
    assert!(state.builds.read(&fixture.commit).is_none());
    assert!(unreachable.published.lock().unwrap().is_empty());

    // Not final: the next call tries again.
    assert!(
        state
            .builds
            .ensure_built(&state, &fixture.commit, &linux_hosts())
            .await
            .is_err()
    );
    assert_eq!(unreachable.count(), 2);

    // A real failure among them is recorded; the unreachable hosts stay missing.
    let back = fixture.install(FakeBuilder::default());
    let state = &fixture.state;
    let result = state
        .builds
        .ensure_built(state, &fixture.commit, &linux_hosts())
        .await
        .unwrap();
    assert_eq!(back.count(), 1);
    assert!(result.failed_hosts(&linux_hosts()).is_empty());
    assert!(
        state
            .builds
            .retry(&fixture.commit)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(state.builds.retry("--force").await.is_err());
}

#[tokio::test]
async fn build_failures_are_final_and_retry_rebuilds_them() {
    let mut fixture = fixture().await;
    let failing = fixture.install(FakeBuilder {
        fail_all: Some("error: builder for '/nix/store/x.drv' failed".into()),
        ..Default::default()
    });
    let state = fixture.state.clone();
    let result = state
        .builds
        .ensure_built(&state, &fixture.commit, &linux_hosts())
        .await
        .unwrap();
    assert_eq!(result.failed_hosts(&linux_hosts()), vec!["a", "b"]);
    state
        .builds
        .ensure_built(&state, &fixture.commit, &linux_hosts())
        .await
        .unwrap();
    assert_eq!(failing.count(), 1, "results are final");
    assert_eq!(
        state.builds.retry(&fixture.commit).await.unwrap(),
        vec!["a", "b"]
    );
    let back = fixture.install(FakeBuilder::default());
    let state = &fixture.state;
    let result = state
        .builds
        .ensure_built(state, &fixture.commit, &linux_hosts())
        .await
        .unwrap();
    assert_eq!(back.count(), 1);
    assert!(result.failed_hosts(&linux_hosts()).is_empty());
}

#[tokio::test]
async fn atomic_write() {
    let mut fixture = fixture().await;
    fixture.install(FakeBuilder::default());
    let state = &fixture.state;
    state
        .builds
        .ensure_built(state, &fixture.commit, &linux_hosts())
        .await
        .unwrap();
    let names: Vec<String> = std::fs::read_dir(fixture.builds_dir())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.contains(&format!("{}.json", fixture.commit)));
    assert!(names.contains(&"channels.json".to_string()));
    assert!(
        !names.iter().any(|name| name.ends_with(".tmp")),
        "{names:?}"
    );
    // Invalid commits are refused before touching the filesystem.
    assert!(
        state
            .builds
            .ensure_built(state, "../escape", &linux_hosts())
            .await
            .is_err()
    );
    assert!(state.builds.read("../escape").is_none());
}

fn read_index(fixture: &Fixture) -> serde_json::Value {
    serde_json::from_str(
        &std::fs::read_to_string(fixture.builds_dir().join("channels.json")).unwrap(),
    )
    .unwrap()
}

/// A new commit on the mirror's `unstable`.
async fn next_commit(fixture: &Fixture, subject: &str) -> String {
    let commit = fixture.origin.commit(subject, "unstable");
    fixture.state.mirror.fetch().await.unwrap();
    commit
}

#[tokio::test]
async fn channel_index_tracks_current_and_previous() {
    let mut fixture = fixture().await;
    let builder = fixture.install(FakeBuilder::default());
    let state = fixture.state.clone();
    let good = fixture.commit.clone();
    state
        .builds
        .ensure_built(&state, &good, &linux_hosts())
        .await
        .unwrap();

    // A failed build does not advance the channel.
    let failed = next_commit(&fixture, "failed").await;
    fixture.install(FakeBuilder {
        fail_all: Some("error: builder for a failed".into()),
        ..Default::default()
    });
    state
        .builds
        .ensure_built(&fixture.state, &failed, &linux_hosts())
        .await
        .unwrap();
    assert_eq!(
        read_index(&fixture),
        serde_json::json!({ "unstable": { "current": good, "previous": null } })
    );

    // Neither does a failed publish.
    let unpublished = next_commit(&fixture, "unpublished").await;
    fixture.install(FakeBuilder {
        publish_error: Some("builder unreachable: ssh: connect refused".into()),
        ..Default::default()
    });
    state
        .builds
        .ensure_built(&fixture.state, &unpublished, &linux_hosts())
        .await
        .unwrap_err();
    assert_eq!(
        read_index(&fixture),
        serde_json::json!({ "unstable": { "current": good, "previous": null } })
    );

    // The next good build moves the last good one to previous.
    let second = next_commit(&fixture, "second").await;
    fixture.state.builder = builder.clone();
    state
        .builds
        .ensure_built(&fixture.state, &second, &linux_hosts())
        .await
        .unwrap();
    let expected = serde_json::json!({ "unstable": { "current": second, "previous": good } });
    assert_eq!(read_index(&fixture), expected);
    assert_eq!(builder.published.lock().unwrap()[1].2, expected);

    // Rebuilding the previous commit after a retry does not move the channel back.
    let mut result = state.builds.read(&good).unwrap();
    result.hosts.get_mut("b").unwrap().ok = false;
    std::fs::write(
        fixture.builds_dir().join(format!("{good}.json")),
        serde_json::to_vec(&result).unwrap(),
    )
    .unwrap();
    assert_eq!(state.builds.retry(&good).await.unwrap(), vec!["b"]);
    state
        .builds
        .ensure_built(&fixture.state, &good, &linux_hosts())
        .await
        .unwrap();
    assert_eq!(builder.count(), 3);
    assert_eq!(read_index(&fixture), expected);

    // A corrupt index fails before anything is built.
    std::fs::write(fixture.builds_dir().join("channels.json"), "{").unwrap();
    let third = next_commit(&fixture, "third").await;
    let error = state
        .builds
        .ensure_built(&fixture.state, &third, &linux_hosts())
        .await
        .unwrap_err();
    assert!(format!("{error:#}").contains("channels.json"), "{error:#}");
    assert_eq!(builder.count(), 3);
    assert!(state.builds.read(&third).is_none());
}

#[tokio::test]
async fn old_results_are_cleaned() {
    let mut fixture = fixture().await;
    fixture.install(FakeBuilder::default());
    let state = &fixture.state;
    let dir = fixture.builds_dir();
    let now = now();
    let commit = |digit: char| std::iter::repeat_n(digit, 40).collect::<String>();
    let (old, kept, recent, locked, previous) = (
        commit('1'),
        commit('2'),
        commit('3'),
        commit('4'),
        commit('5'),
    );
    for name in [&old, &kept, &recent, &locked, &previous] {
        state
            .builds
            .record_built(state, name, BTreeMap::from([("a".to_string(), ok("a"))]))
            .await
            .unwrap();
    }
    for name in [&old, &kept, &locked, &previous] {
        set_age(&dir.join(format!("{name}.json")), now, 31);
    }
    set_age(&dir.join(format!("{recent}.json")), now, 29);
    std::fs::write(
        dir.join("channels.json"),
        serde_json::json!({ "stable": { "current": kept, "previous": previous } }).to_string(),
    )
    .unwrap();
    let held = lock_file(&dir.join(format!("{locked}.lock")));
    state.builds.cleanup(now).await.unwrap();
    let exists = |name: &str, extension: &str| dir.join(format!("{name}.{extension}")).exists();
    assert!(!exists(&old, "json") && !exists(&old, "lock"));
    for name in [&kept, &recent, &locked, &previous] {
        assert!(exists(name, "json"), "{name}");
    }
    drop(held);

    // Every successful `ensure_built` cleans up too. (A child process of another test may hold a
    // copy of the dropped lock for a moment between fork and exec; cleanup skips it then.)
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while exists(&locked, "json") && std::time::Instant::now() < deadline {
        state.builds.cleanup(now).await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(!exists(&locked, "json"));
    set_age(&dir.join(format!("{recent}.json")), now, 40);
    state
        .builds
        .ensure_built(state, &fixture.commit, &linux_hosts())
        .await
        .unwrap();
    assert!(!exists(&recent, "json") && !exists(&recent, "lock"));
    assert!(exists(&kept, "json") && exists(&fixture.commit, "json"));
}

async fn await_all(handles: Vec<tokio::task::JoinHandle<()>>) {
    for handle in handles {
        handle.await.unwrap();
    }
}

#[tokio::test]
async fn prebuild_runs_once_per_new_head() {
    let mut fixture = fixture().await;
    let builder = Arc::new(CommitBuilder::default());
    fixture.state.builder = builder.clone();
    let state = &fixture.state;
    let linux = vec!["a".to_string(), "b".to_string()];

    // Server start: every channel's current head once.
    state.mirror.fetch().await.unwrap();
    let handles = prebuild_new_heads(state).await;
    assert_eq!(handles.len(), 1);
    await_all(handles).await;
    assert_eq!(
        builder.calls(),
        vec![(fixture.commit.clone(), linux.clone())]
    );

    // A fetch that moves nothing prebuilds nothing.
    state.mirror.fetch().await.unwrap();
    assert!(prebuild_new_heads(state).await.is_empty());

    let next = fixture.origin.commit("next", "unstable");
    state.mirror.fetch().await.unwrap();
    let handles = prebuild_new_heads(state).await;
    assert_eq!(handles.len(), 1);
    await_all(handles).await;
    state.mirror.fetch().await.unwrap();
    assert!(prebuild_new_heads(state).await.is_empty());
    assert_eq!(
        builder.calls(),
        vec![(fixture.commit.clone(), linux.clone()), (next, linux)]
    );
    assert!(state.building.lock().unwrap().is_empty());
}

#[tokio::test]
async fn queued_prebuilds_collapse_to_the_newest_head() {
    let mut fixture = fixture().await;
    let hold = Arc::new(Semaphore::new(0));
    let builder = Arc::new(CommitBuilder::held(hold.clone()));
    fixture.state.builder = builder.clone();
    let first = fixture.commit.clone();
    let second = fixture.origin.commit("second", "unstable");
    let third = fixture.origin.commit("third", "unstable");
    let state = &fixture.state;

    let running = spawn_prebuild(state, "unstable", &first).unwrap();
    builder.entered.notified().await;
    assert_eq!(state.building.lock().unwrap().get("unstable"), Some(&first));
    let queued = spawn_prebuild(state, "unstable", &second).expect("nothing was waiting");
    assert!(
        spawn_prebuild(state, "unstable", &third).is_none(),
        "one waiting task per channel"
    );
    hold.add_permits(2);
    running.await.unwrap();
    queued.await.unwrap();

    let built: Vec<String> = builder
        .calls()
        .into_iter()
        .map(|(commit, _)| commit)
        .collect();
    assert_eq!(built, vec![first, third]);
    assert!(state.building.lock().unwrap().is_empty());
}

// ---- the channel index follows each channel's head (C1/I5) ----

fn sorted_paths(names: &[&str]) -> Vec<String> {
    let mut paths: Vec<String> = names.iter().map(|name| store_path(name)).collect();
    paths.sort();
    paths
}

fn sorted(mut paths: Vec<String>) -> Vec<String> {
    paths.sort();
    paths
}

#[tokio::test]
async fn an_updater_record_spanning_two_channels_advances_each_at_its_head() {
    let mut fixture = fixture_with_hosts(vec![
        host("a", "linux"),
        host("b", "linux"),
        on("stable", host("s", "linux")),
    ])
    .await;
    fixture.origin.push("stable");
    fixture.state.mirror.fetch().await.unwrap();
    let builder = Arc::new(CommitBuilder::default());
    fixture.state.builder = builder.clone();
    let state = &fixture.state;
    let commit = fixture.commit.clone();

    // The updater built the changed hosts a (unstable) and s (stable) at the pushed commit.
    let results = state
        .builds
        .build_dir(
            state,
            &fixture.origin.work,
            &[host("a", "linux"), on("stable", host("s", "linux"))],
        )
        .await;
    state
        .builds
        .record_built(state, &commit, results)
        .await
        .unwrap();
    // stable has every host built at its head; unstable still misses b.
    assert_eq!(
        read_index(&fixture),
        serde_json::json!({ "stable": { "current": commit, "previous": null } })
    );
    let published = builder.published();
    assert_eq!(published.len(), 1);
    assert_eq!(sorted(published[0].1.clone()), sorted_paths(&["a", "s"]));

    // The prebuild of unstable builds b and publishes every host of the channel.
    state
        .builds
        .ensure_built(state, &commit, &[host("a", "linux"), host("b", "linux")])
        .await
        .unwrap();
    assert_eq!(
        read_index(&fixture),
        serde_json::json!({
            "stable": { "current": commit, "previous": null },
            "unstable": { "current": commit, "previous": null },
        })
    );
    let published = builder.published();
    assert_eq!(published.len(), 2);
    assert_eq!(published[1].0, commit);
    assert_eq!(sorted(published[1].1.clone()), sorted_paths(&["a", "b"]));
}

#[tokio::test]
async fn a_recorded_commit_advances_once_it_is_the_head() {
    let mut fixture = fixture().await;
    let builder = Arc::new(CommitBuilder::default());
    fixture.state.builder = builder.clone();
    let state = fixture.state.clone();
    let first = fixture.commit.clone();
    state
        .builds
        .ensure_built(&state, &first, &linux_hosts())
        .await
        .unwrap();
    assert_eq!(
        read_index(&fixture),
        serde_json::json!({ "unstable": { "current": first, "previous": null } })
    );

    // Recorded before the mirror saw it: not the head, so the channel stays.
    let second = fixture.origin.commit("second", "unstable");
    state
        .builds
        .record_built(
            &state,
            &second,
            BTreeMap::from([("a".to_string(), ok("a")), ("b".to_string(), ok("b"))]),
        )
        .await
        .unwrap();
    assert_eq!(
        read_index(&fixture),
        serde_json::json!({ "unstable": { "current": first, "previous": null } })
    );
    assert_eq!(builder.published().len(), 2);

    // Once it is the head, the gate realises every host again (instant on the builder while it
    // has the paths), advances and publishes every host again.
    state.mirror.fetch().await.unwrap();
    state
        .builds
        .ensure_built(&state, &second, &linux_hosts())
        .await
        .unwrap();
    let linux = vec!["a".to_string(), "b".to_string()];
    assert_eq!(builder.calls().len(), 2);
    assert_eq!(builder.calls()[1], (second.clone(), linux));
    let expected = serde_json::json!({ "unstable": { "current": second, "previous": first } });
    assert_eq!(read_index(&fixture), expected);
    let published = builder.published();
    assert_eq!(published.len(), 3);
    assert_eq!(published[2].0, second);
    assert_eq!(sorted(published[2].1.clone()), sorted_paths(&["a", "b"]));

    // Calling again builds, changes and publishes nothing.
    state
        .builds
        .ensure_built(&state, &second, &linux_hosts())
        .await
        .unwrap();
    assert_eq!(builder.calls().len(), 2);
    assert_eq!(builder.published().len(), 3);
}

#[tokio::test]
async fn a_commit_that_is_not_the_head_never_advances() {
    let mut fixture = fixture().await;
    let builder = Arc::new(CommitBuilder::default());
    fixture.state.builder = builder.clone();
    let state = fixture.state.clone();
    let first = fixture.commit.clone();
    let older = fixture.origin.commit("older", "unstable");
    let head = next_commit(&fixture, "head").await;

    // A re-send of an older desired apply builds it; the channel does not move to it.
    state
        .builds
        .ensure_built(&state, &older, &linux_hosts())
        .await
        .unwrap();
    assert!(!fixture.builds_dir().join("channels.json").exists());
    assert_eq!(builder.published().len(), 1, "its new paths are published");

    state
        .builds
        .ensure_built(&state, &head, &linux_hosts())
        .await
        .unwrap();
    let expected = serde_json::json!({ "unstable": { "current": head, "previous": null } });
    assert_eq!(read_index(&fixture), expected);

    // Older commits built later never push the head out.
    for commit in [&first, &older] {
        state
            .builds
            .ensure_built(&state, commit, &linux_hosts())
            .await
            .unwrap();
    }
    assert_eq!(read_index(&fixture), expected);
}

#[tokio::test]
async fn a_retry_publishes_the_hosts_that_were_ok_before() {
    let mut fixture = fixture().await;
    let builder = Arc::new(CommitBuilder::failing(&["b"]));
    fixture.state.builder = builder.clone();
    let state = fixture.state.clone();
    let commit = fixture.commit.clone();
    let result = state
        .builds
        .ensure_built(&state, &commit, &linux_hosts())
        .await
        .unwrap();
    assert_eq!(result.failed_hosts(&linux_hosts()), vec!["b"]);
    assert_eq!(
        builder.published(),
        vec![(commit.clone(), vec![store_path("a")])]
    );
    assert!(!fixture.builds_dir().join("channels.json").exists());

    assert_eq!(state.builds.retry(&commit).await.unwrap(), vec!["b"]);
    builder.fail.lock().unwrap().clear();
    state
        .builds
        .ensure_built(&state, &commit, &linux_hosts())
        .await
        .unwrap();
    let published = builder.published();
    assert_eq!(published.len(), 2);
    assert_eq!(sorted(published[1].1.clone()), sorted_paths(&["a", "b"]));
    assert_eq!(
        read_index(&fixture),
        serde_json::json!({ "unstable": { "current": commit, "previous": null } })
    );
}

#[tokio::test]
async fn a_failed_index_write_is_a_gate_error() {
    let mut fixture = fixture().await;
    let builder = Arc::new(CommitBuilder::default());
    fixture.state.builder = builder.clone();
    let state = fixture.state.clone();
    // The index is written via `channels.json.tmp`; a directory there makes the write fail.
    std::fs::create_dir_all(fixture.builds_dir().join("channels.json.tmp")).unwrap();
    let error = state
        .builds
        .ensure_built(&state, &fixture.commit, &linux_hosts())
        .await
        .unwrap_err();
    assert!(format!("{error:#}").contains("channels.json"), "{error:#}");
    assert_eq!(builder.published().len(), 1);
}

#[tokio::test]
async fn a_prebuild_after_the_index_was_removed_republishes_the_heads() {
    let mut fixture = fixture().await;
    let builder = Arc::new(CommitBuilder::default());
    fixture.state.builder = builder.clone();
    let state = &fixture.state;
    let commit = fixture.commit.clone();
    state
        .builds
        .ensure_built(state, &commit, &linux_hosts())
        .await
        .unwrap();
    assert_eq!(builder.published().len(), 1);

    // The cache was emptied: the operator removes the index and restarts the server, whose
    // first prebuild round realises every head again and republishes it.
    std::fs::remove_file(fixture.builds_dir().join("channels.json")).unwrap();
    await_all(prebuild_new_heads(state).await).await;
    assert_eq!(builder.calls().len(), 2);
    assert_eq!(
        builder.calls()[1],
        (commit.clone(), vec!["a".to_string(), "b".to_string()])
    );
    let published = builder.published();
    assert_eq!(published.len(), 2);
    assert_eq!(sorted(published[1].1.clone()), sorted_paths(&["a", "b"]));
    assert_eq!(
        read_index(&fixture),
        serde_json::json!({ "unstable": { "current": commit, "previous": null } })
    );
}

// ---- an advance re-realises every path it publishes (the builder's GC keeps none) ----

#[tokio::test]
async fn a_retry_re_realises_the_paths_the_builder_collected() {
    let mut fixture = fixture().await;
    let builder = Arc::new(CommitBuilder::failing(&["b"]));
    builder.keep_store();
    fixture.state.builder = builder.clone();
    let state = fixture.state.clone();
    let commit = fixture.commit.clone();
    assert_eq!(
        build_channel(&state, "unstable", &commit).await.unwrap(),
        vec!["b"]
    );
    assert_eq!(
        builder.published(),
        vec![(commit.clone(), vec![path_of("a")])]
    );

    // A week later the builder's GC has collected a's unrooted path; the retry of b is
    // followed by an apply.
    builder.forget("a");
    assert_eq!(state.builds.retry(&commit).await.unwrap(), vec!["b"]);
    builder.fail.lock().unwrap().clear();
    assert!(
        build_channel(&state, "unstable", &commit)
            .await
            .unwrap()
            .is_empty()
    );
    let linux = vec!["a".to_string(), "b".to_string()];
    assert_eq!(builder.calls().last(), Some(&(commit.clone(), linux)));
    let published = builder.published();
    assert_eq!(published.len(), 2);
    assert_eq!(sorted(published[1].1.clone()), sorted_paths(&["a", "b"]));
    assert_eq!(
        read_index(&fixture),
        serde_json::json!({ "unstable": { "current": commit, "previous": null } })
    );
}

#[tokio::test]
async fn a_channel_reset_to_an_earlier_commit_republishes_it_once() {
    let mut fixture = fixture().await;
    let builder = Arc::new(CommitBuilder::default());
    builder.keep_store();
    fixture.state.builder = builder.clone();
    let state = fixture.state.clone();
    let earlier = fixture.commit.clone();
    state.mirror.fetch().await.unwrap();
    await_all(prebuild_new_heads(&state).await).await;
    let later = next_commit(&fixture, "later").await;
    await_all(prebuild_new_heads(&state).await).await;
    assert_eq!(
        read_index(&fixture),
        serde_json::json!({ "unstable": { "current": later, "previous": earlier } })
    );
    assert_eq!(builder.calls().len(), 2);
    assert_eq!(builder.published().len(), 2);

    // The channel is reset to the earlier commit after the builder collected its paths.
    builder.forget("a");
    builder.forget("b");
    git(&fixture.origin.work, &["reset", "-q", "--hard", &earlier]);
    fixture.origin.push("unstable");
    state.mirror.fetch().await.unwrap();
    await_all(prebuild_new_heads(&state).await).await;
    let linux = vec!["a".to_string(), "b".to_string()];
    assert_eq!(builder.calls().len(), 3);
    assert_eq!(builder.calls()[2], (earlier.clone(), linux));
    let published = builder.published();
    assert_eq!(published.len(), 3);
    assert_eq!(published[2].0, earlier);
    assert_eq!(sorted(published[2].1.clone()), sorted_paths(&["a", "b"]));
    let expected = serde_json::json!({ "unstable": { "current": earlier, "previous": later } });
    assert_eq!(read_index(&fixture), expected);

    // With the earlier commit in the index, later rounds and applies do nothing.
    state.mirror.fetch().await.unwrap();
    assert!(prebuild_new_heads(&state).await.is_empty());
    assert!(
        build_channel(&state, "unstable", &earlier)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(builder.calls().len(), 3);
    assert_eq!(builder.published().len(), 3);
    assert_eq!(read_index(&fixture), expected);
}

#[tokio::test]
async fn an_unreachable_builder_during_re_realisation_advances_nothing() {
    let mut fixture = fixture().await;
    let builder = Arc::new(CommitBuilder::default());
    fixture.state.builder = builder.clone();
    let state = fixture.state.clone();
    let first = fixture.commit.clone();
    state
        .builds
        .ensure_built(&state, &first, &linux_hosts())
        .await
        .unwrap();
    // Recorded before the mirror saw it, then it becomes the head.
    let second = fixture.origin.commit("second", "unstable");
    let recorded = state
        .builds
        .record_built(
            &state,
            &second,
            BTreeMap::from([("a".to_string(), ok("a")), ("b".to_string(), ok("b"))]),
        )
        .await
        .unwrap();
    state.mirror.fetch().await.unwrap();
    let published = builder.published().len();

    builder.set_unreachable(true);
    let error = state
        .builds
        .ensure_built(&state, &second, &linux_hosts())
        .await
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("builder unreachable"),
        "{error:#}"
    );
    let linux = vec!["a".to_string(), "b".to_string()];
    assert_eq!(builder.calls().last(), Some(&(second.clone(), linux)));
    assert_eq!(
        state.builds.read(&second),
        Some(recorded),
        "nothing recorded"
    );
    assert_eq!(builder.published().len(), published, "nothing published");
    assert_eq!(
        read_index(&fixture),
        serde_json::json!({ "unstable": { "current": first, "previous": null } })
    );
    assert!(
        !fixture.builds_dir().join("work").join(&second).exists(),
        "worktree removed"
    );

    // Once the builder answers, the next call re-realises, publishes and advances.
    builder.set_unreachable(false);
    state
        .builds
        .ensure_built(&state, &second, &linux_hosts())
        .await
        .unwrap();
    assert_eq!(
        read_index(&fixture),
        serde_json::json!({ "unstable": { "current": second, "previous": first } })
    );
}

#[tokio::test]
async fn a_host_that_fails_on_re_realisation_is_recorded_as_failed() {
    let mut fixture = fixture().await;
    let builder = Arc::new(CommitBuilder::default());
    fixture.state.builder = builder.clone();
    let state = fixture.state.clone();
    let first = fixture.commit.clone();
    state
        .builds
        .ensure_built(&state, &first, &linux_hosts())
        .await
        .unwrap();
    let second = fixture.origin.commit("second", "unstable");
    state
        .builds
        .record_built(
            &state,
            &second,
            BTreeMap::from([("a".to_string(), ok("a")), ("b".to_string(), ok("b"))]),
        )
        .await
        .unwrap();
    state.mirror.fetch().await.unwrap();
    let published = builder.published().len();

    builder.fail.lock().unwrap().insert("b".into());
    let result = state
        .builds
        .ensure_built(&state, &second, &linux_hosts())
        .await
        .unwrap();
    assert_eq!(result.failed_hosts(&linux_hosts()), vec!["b"]);
    assert_eq!(result.hosts["a"], ok("a"), "an ok host keeps its entry");
    assert_eq!(state.builds.read(&second), Some(result));
    assert_eq!(builder.published().len(), published, "nothing published");
    assert_eq!(
        read_index(&fixture),
        serde_json::json!({ "unstable": { "current": first, "previous": null } })
    );
}

// ---- builds that could not run back off (I1) ----

#[test]
fn backoff_doubles_from_30_seconds_to_10_minutes() {
    let backoff = BuildBackoff::default();
    let commit = "1".repeat(40);
    assert!(backoff.ready_at("stable", &commit, 0));
    assert!(!backoff.waiting("stable", &commit));
    let mut now = 1_000;
    for delay in [30, 60, 120, 240, 480, 600, 600] {
        backoff.failed_at("stable", &commit, now);
        assert!(backoff.waiting("stable", &commit));
        assert!(
            !backoff.ready_at("stable", &commit, now + delay - 1),
            "{delay}"
        );
        assert!(backoff.ready_at("stable", &commit, now + delay), "{delay}");
        now += delay;
    }
    // Per channel and commit.
    assert!(backoff.ready_at("unstable", &commit, now));
    assert!(backoff.ready_at("stable", &"2".repeat(40), now));
    backoff.succeeded("stable", &commit);
    assert!(!backoff.waiting("stable", &commit));
    assert!(backoff.ready_at("stable", &commit, 0));
}

#[tokio::test]
async fn an_unreachable_prebuild_backs_off_then_retries() {
    let mut fixture = fixture().await;
    let builder = Arc::new(CommitBuilder::default());
    builder.set_unreachable(true);
    fixture.state.builder = builder.clone();
    let commit = fixture.commit.clone();
    {
        let state = &fixture.state;
        state.mirror.fetch().await.unwrap();
        await_all(prebuild_new_heads(state).await).await;
        assert_eq!(builder.calls().len(), 1);
        assert!(state.builds.read(&commit).is_none());
        assert!(state.build_backoff.waiting("unstable", &commit));
        // The head did not move and the backoff has not passed: no new attempt.
        state.mirror.fetch().await.unwrap();
        assert!(prebuild_new_heads(state).await.is_empty());
        assert_eq!(builder.calls().len(), 1);
    }

    // Without a delay the next fetch retries the head; once it builds, the backoff ends.
    fixture.state.build_backoff = Arc::new(BuildBackoff::new(Duration::ZERO, Duration::ZERO));
    let state = &fixture.state;
    state.build_backoff.failed("unstable", &commit);
    builder.set_unreachable(false);
    let handles = prebuild_new_heads(state).await;
    assert_eq!(handles.len(), 1);
    await_all(handles).await;
    assert_eq!(builder.calls().len(), 2);
    assert!(state.builds.read(&commit).unwrap().hosts["a"].ok);
    assert!(!state.build_backoff.waiting("unstable", &commit));
    assert!(prebuild_new_heads(state).await.is_empty());
}

#[tokio::test]
async fn stale_build_worktrees_are_removed_at_start() {
    let fixture = fixture().await;
    let state = &fixture.state;
    let work = fixture.builds_dir().join("work");
    // A server killed mid-build leaves its worktree (registered in the mirror) and stray files.
    state
        .mirror
        .worktree_at(&fixture.commit, &work.join(&fixture.commit))
        .await
        .unwrap();
    std::fs::create_dir_all(work.join("junk")).unwrap();
    let mirror = state.config.state_dir.join("mirror.git");
    let worktrees = || git(&mirror, &["worktree", "list", "--porcelain"]);
    assert!(worktrees().contains(&fixture.commit));

    state.builds.clear_stale_work(&state.mirror).await;
    assert!(!work.exists() || std::fs::read_dir(&work).unwrap().next().is_none());
    assert!(!worktrees().contains(&fixture.commit), "{}", worktrees());
}
