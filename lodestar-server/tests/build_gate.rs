//! The build gate against a fake builder and a bare-repo origin (no network, no real Nix).

mod support;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use lodestar_server::builds::{
    BuildGate, Builder, HostBuild, HostResult, prebuild_new_heads, spawn_prebuild,
};
use lodestar_server::config::{AccessConfig, ManagedHost, OnlineSource, ServerConfig};
use lodestar_server::db;
use lodestar_server::mirror::Mirror;
use lodestar_server::state::AppState;
use lodestar_server::tailscale::StaticOnline;
use support::{FakeBuilder as CommitBuilder, Origin, git};
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
    let hosts = vec![host("a", "linux"), host("b", "linux"), host("c", "darwin")];
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

fn lock_is_free(path: &Path) -> bool {
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.try_lock().is_ok()
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
    assert_eq!(
        builder.built_hosts.lock().unwrap().as_slice(),
        &[vec!["b".to_string()]]
    );
    assert_eq!(result.hosts.keys().collect::<Vec<_>>(), vec!["a", "b"]);
    assert_eq!(result.hosts["a"].built_at, 1, "existing entry kept");
    // Each call published only its new path, with the channel index.
    let published = builder.published.lock().unwrap();
    let index = serde_json::json!({ "unstable": { "current": fixture.commit, "previous": null } });
    assert_eq!(published.len(), 2);
    assert_eq!(published[0].0, fixture.commit);
    assert_eq!(published[0].1, vec![store_path("a")]);
    assert_eq!(published[0].2, index);
    assert_eq!(published[1].1, vec![store_path("b")]);
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

    // A failing publish fails the recorded hosts.
    let failing = fixture.install(FakeBuilder {
        publish_error: Some("publishing failed: disk full".into()),
        ..Default::default()
    });
    let state = &fixture.state;
    let other = "a".repeat(40);
    let result = state
        .builds
        .record_built(state, &other, BTreeMap::from([("a".to_string(), ok("a"))]))
        .await
        .unwrap();
    assert_eq!(failing.published.lock().unwrap().len(), 1);
    assert_eq!(result.failed_hosts(&linux_hosts()), vec!["a"]);
    assert!(
        result.hosts["a"]
            .log_tail
            .as_deref()
            .unwrap()
            .starts_with("publish failed: ")
    );
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

    // A failing publish: the built hosts are recorded as failed.
    let failing = fixture.install(FakeBuilder {
        publish_error: Some("builder unreachable: ssh: connect refused".into()),
        ..Default::default()
    });
    let state = &fixture.state;
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        state
            .builds
            .ensure_built(state, &fixture.commit, &linux_hosts()),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(failing.count(), 1);
    assert_eq!(result.failed_hosts(&linux_hosts()), vec!["a", "b"]);
    for name in ["a", "b"] {
        let tail = result.hosts[name].log_tail.as_deref().unwrap();
        assert!(tail.starts_with("publish failed: "), "{tail}");
        assert!(tail.contains("builder unreachable"), "{tail}");
    }
    assert_eq!(
        serde_json::to_value(state.builds.read(&fixture.commit).unwrap()).unwrap(),
        serde_json::to_value(&result).unwrap()
    );
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
async fn builder_unreachable_is_a_failure_for_every_host() {
    let mut fixture = fixture().await;
    let unreachable = fixture.install(FakeBuilder {
        fail_all: Some("builder unreachable: error: cannot connect to 'alice@example.com'".into()),
        ..Default::default()
    });
    let state = fixture.state.clone();
    let result = state
        .builds
        .ensure_built(&state, &fixture.commit, &linux_hosts())
        .await
        .unwrap();
    assert_eq!(result.failed_hosts(&linux_hosts()), vec!["a", "b"]);
    for name in ["a", "b"] {
        assert!(
            result.hosts[name]
                .log_tail
                .as_deref()
                .unwrap()
                .starts_with("builder unreachable")
        );
    }
    assert!(unreachable.published.lock().unwrap().is_empty());

    // Results are final: calling again builds nothing.
    state
        .builds
        .ensure_built(&state, &fixture.commit, &linux_hosts())
        .await
        .unwrap();
    assert_eq!(unreachable.count(), 1);

    // Retry removes the failed hosts; the next call builds them again.
    assert_eq!(
        state.builds.retry(&fixture.commit).await.unwrap(),
        vec!["a", "b"]
    );
    assert!(state.builds.read(&fixture.commit).unwrap().hosts.is_empty());
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
        .unwrap();
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

    // Every successful `ensure_built` cleans up too.
    state.builds.cleanup(now).await.unwrap();
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

    let running = spawn_prebuild(state, "unstable", &first);
    builder.entered.notified().await;
    assert_eq!(state.building.lock().unwrap().get("unstable"), Some(&first));
    let queued = [
        spawn_prebuild(state, "unstable", &second),
        spawn_prebuild(state, "unstable", &third),
    ];
    hold.add_permits(2);
    running.await.unwrap();
    await_all(queued.into()).await;

    let built: Vec<String> = builder
        .calls()
        .into_iter()
        .map(|(commit, _)| commit)
        .collect();
    assert_eq!(built, vec![first, third]);
    assert!(state.building.lock().unwrap().is_empty());
}
