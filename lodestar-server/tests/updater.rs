//! The updater against a scripted fake Nix and a bare-repo origin (no network, no real Nix).

mod support;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use lodestar_server::builds::{Builder, HostBuild};
use lodestar_server::config::{
    AccessConfig, ManagedHost, OnlineSource, ServerConfig, SoftwareConfig, UpdatesConfig, Upstream,
};
use lodestar_server::db;
use lodestar_server::mirror::Mirror;
use lodestar_server::state::AppState;
use lodestar_server::tailscale::StaticOnline;
use lodestar_server::updates::nix_runner::NixRunner;
use lodestar_server::updates::update_run::{UpdateOutcome, UpdateRun, VersionChange};
use lodestar_server::updates::updater::{commit_message, restore_last, run_all, run_input};
use lodestar_server::updates::updater_loop::{newest_finished, next_run_at};
use support::{Origin, git};
use tokio::sync::Notify;

const INPUT: &str = "nixpkgs-unstable";
const OLD_LOCK: &str = "{\"old\":true}\n";
const NEW_LOCK: &str = "{\"new\":true}\n";

type Versions = BTreeMap<String, String>;

fn versions(pairs: &[(&str, &str)]) -> Versions {
    pairs
        .iter()
        .map(|(software, version)| (software.to_string(), version.to_string()))
        .collect()
}

/// Scripted Nix: `flake_update` writes `new_lock` (if any); `eval_versions` answers from
/// `before` or `after` depending on whether the worktree's lock is still the old one.
#[derive(Default)]
struct FakeNix {
    new_lock: Option<String>,
    before: HashMap<String, Result<Versions, String>>,
    after: HashMap<String, Result<Versions, String>>,
    failing_builds: HashSet<String>,
    built: Mutex<Vec<String>>,
    updated: Mutex<Vec<String>>,
    /// Runs inside `build` (e.g. moves the channel on the origin).
    on_build: Option<Box<dyn Fn() + Send + Sync>>,
    /// `(entered, release)`: `flake_update` signals `entered` and waits for `release`.
    gate: Option<(Arc<Notify>, Arc<Notify>)>,
    /// `flake_update` deletes `flake.lock` instead of writing `new_lock`.
    delete_lock: bool,
    /// `eval_versions` calls after `flake_update`.
    evals_after_update: Mutex<u32>,
}

#[async_trait]
impl NixRunner for FakeNix {
    async fn flake_update(&self, dir: &Path, input: &str) -> anyhow::Result<()> {
        self.updated.lock().unwrap().push(input.to_string());
        if let Some((entered, release)) = &self.gate {
            entered.notify_one();
            release.notified().await;
        }
        if self.delete_lock {
            std::fs::remove_file(dir.join("flake.lock"))?;
        } else if let Some(lock) = &self.new_lock {
            std::fs::write(dir.join("flake.lock"), lock)?;
        }
        Ok(())
    }

    async fn eval_versions(&self, dir: &Path, host: &ManagedHost) -> anyhow::Result<Versions> {
        if !self.updated.lock().unwrap().is_empty() {
            *self.evals_after_update.lock().unwrap() += 1;
        }
        // A missing lock reads as "after" (nix would lock the inputs afresh).
        let lock = std::fs::read_to_string(dir.join("flake.lock")).unwrap_or_default();
        let phase = if lock == OLD_LOCK {
            &self.before
        } else {
            &self.after
        };
        match phase.get(&host.name) {
            Some(Ok(versions)) => Ok(versions.clone()),
            Some(Err(log)) => Err(anyhow::anyhow!("{log}")),
            None => Ok(Versions::new()),
        }
    }

    async fn build(&self, _dir: &Path, host: &ManagedHost) -> Result<(), String> {
        self.built.lock().unwrap().push(host.name.clone());
        if let Some(hook) = &self.on_build {
            hook();
        }
        if self.failing_builds.contains(&host.name) {
            return Err(format!("error: builder for {} failed", host.name));
        }
        Ok(())
    }
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
        node_id: None,
        dns_name: None,
    }
}

struct Fixture {
    state: AppState,
    origin: Origin,
    base: String,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn state_dir(&self) -> PathBuf {
        self.state.config.state_dir.clone()
    }

    fn remote_head(&self) -> String {
        self.origin.bare_ref("unstable")
    }
}

async fn fixture() -> Fixture {
    fixture_with(false).await
}

async fn fixture_with(builds: bool) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    std::fs::write(origin.work.join("flake.lock"), OLD_LOCK).unwrap();
    git(&origin.work, &["add", "flake.lock"]);
    let base = origin.commit("lock", "unstable");
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
        updates: UpdatesConfig {
            software: BTreeMap::from([(
                "tailscale".to_string(),
                SoftwareConfig {
                    input: INPUT.into(),
                    upstream: Upstream::Github("tailscale/tailscale".into()),
                },
            )]),
            ..Default::default()
        },
        builds: lodestar_server::config::BuildsConfig {
            enable: builds,
            ..Default::default()
        },
        metrics: None,
        nix_bin: "nix".into(),
    };
    let connection = db::connect(&config.state_dir).await.unwrap();
    let mirror = Arc::new(Mirror::open_or_init(&config).await.unwrap());
    let online = Arc::new(StaticOnline::new(&HashMap::new()).unwrap());
    let hosts = vec![host("a", "linux"), host("b", "darwin")];
    let state = AppState::new(Arc::new(config), connection, mirror, online, hosts).unwrap();
    Fixture {
        state,
        origin,
        base,
        _dir: dir,
    }
}

fn install(fixture: &mut Fixture, nix: FakeNix) -> Arc<FakeNix> {
    let nix = Arc::new(nix);
    fixture.state.nix = nix.clone();
    nix
}

/// The versions both hosts report before the bump: a 1.98.10, b 1.98.10.
fn before() -> HashMap<String, Result<Versions, String>> {
    HashMap::from([
        ("a".into(), Ok(versions(&[("tailscale", "1.98.10")]))),
        ("b".into(), Ok(versions(&[("tailscale", "1.98.10")]))),
    ])
}

async fn update_audits(state: &AppState) -> Vec<serde_json::Value> {
    db::recent_audit(&state.db, 50)
        .await
        .unwrap()
        .into_iter()
        .filter(|event| event.kind == "update")
        .inspect(|event| {
            assert_eq!(event.actor, "lodestar");
            assert_eq!(event.host, None);
        })
        .map(|event| event.details)
        .collect()
}

fn software() -> Vec<String> {
    vec!["tailscale".to_string()]
}

#[test]
fn commit_message_lists_changes_in_software_order() {
    let change = |software: &str, from: &str, to: &str| VersionChange {
        software: software.into(),
        from: from.into(),
        to: to.into(),
    };
    assert_eq!(
        commit_message(INPUT, &[change("tailscale", "1.98.10", "1.104.2")]),
        "chore(nixpkgs-unstable): bump for tailscale 1.98.10 -> 1.104.2"
    );
    assert_eq!(
        commit_message(
            INPUT,
            &[
                change("tailscale", "1.98.10", "1.104.2"),
                change("jellyfin", "10.10.1", "10.11.0"),
            ]
        ),
        "chore(nixpkgs-unstable): bump for jellyfin 10.10.1 -> 10.11.0, tailscale 1.98.10 -> 1.104.2"
    );
}

#[tokio::test]
async fn unchanged_lock_is_no_change() {
    let mut fixture = fixture().await;
    let nix = install(
        &mut fixture,
        FakeNix {
            before: before(),
            ..Default::default()
        },
    );
    let run = run_input(&fixture.state, INPUT, &software()).await;
    assert_eq!(run.input, INPUT);
    assert_eq!(run.outcome, UpdateOutcome::NoChange);
    assert_eq!(*nix.updated.lock().unwrap(), vec![INPUT.to_string()]);
    assert!(nix.built.lock().unwrap().is_empty());
    assert_eq!(fixture.remote_head(), fixture.base);
    let audits = update_audits(&fixture.state).await;
    assert_eq!(audits.len(), 1);
    assert_eq!(audits[0]["input"], INPUT);
    assert_eq!(audits[0]["result"], "no-change");
    assert!(!fixture.state_dir().join("updater").join(INPUT).exists());
    assert_eq!(fixture.state.updates.last.lock().unwrap()[INPUT], run);
}

#[tokio::test]
async fn lock_churn_without_newer_versions_is_no_change() {
    let mut fixture = fixture().await;
    let nix = install(
        &mut fixture,
        FakeNix {
            new_lock: Some(NEW_LOCK.into()),
            before: before(),
            // Untracked software moved; tracked did not.
            after: HashMap::from([
                (
                    "a".into(),
                    Ok(versions(&[("tailscale", "1.98.10"), ("other", "2")])),
                ),
                ("b".into(), Ok(versions(&[("tailscale", "1.98.10")]))),
            ]),
            ..Default::default()
        },
    );
    let run = run_input(&fixture.state, INPUT, &software()).await;
    assert_eq!(run.outcome, UpdateOutcome::NoChange);
    assert!(nix.built.lock().unwrap().is_empty());
    assert_eq!(fixture.remote_head(), fixture.base);
    assert_eq!(
        update_audits(&fixture.state).await[0]["result"],
        "no-change"
    );
    assert!(!fixture.state_dir().join("updater").join(INPUT).exists());
}

#[tokio::test]
async fn downgrade_is_no_change_with_a_note() {
    let mut fixture = fixture().await;
    let nix = install(
        &mut fixture,
        FakeNix {
            new_lock: Some(NEW_LOCK.into()),
            before: before(),
            after: HashMap::from([
                ("a".into(), Ok(versions(&[("tailscale", "1.104.2")]))),
                ("b".into(), Ok(versions(&[("tailscale", "1.96.0")]))),
            ]),
            ..Default::default()
        },
    );
    let run = run_input(&fixture.state, INPUT, &software()).await;
    assert_eq!(run.outcome, UpdateOutcome::NoChange);
    assert!(nix.built.lock().unwrap().is_empty());
    assert_eq!(fixture.remote_head(), fixture.base);
    let audits = update_audits(&fixture.state).await;
    assert_eq!(audits[0]["result"], "no-change");
    assert_eq!(audits[0]["note"], "downgrade");
}

#[tokio::test]
async fn newer_version_on_one_host_is_built_and_pushed() {
    let mut fixture = fixture().await;
    let nix = install(
        &mut fixture,
        FakeNix {
            new_lock: Some(NEW_LOCK.into()),
            before: before(),
            after: HashMap::from([
                ("a".into(), Ok(versions(&[("tailscale", "1.104.2")]))),
                ("b".into(), Ok(versions(&[("tailscale", "1.98.10")]))),
            ]),
            ..Default::default()
        },
    );
    let run = run_input(&fixture.state, INPUT, &software()).await;
    let changes = vec![VersionChange {
        software: "tailscale".into(),
        from: "1.98.10".into(),
        to: "1.104.2".into(),
    }];
    let UpdateOutcome::Pushed {
        commit,
        changes: pushed,
    } = &run.outcome
    else {
        panic!("expected Pushed, got {:?}", run.outcome);
    };
    assert_eq!(pushed, &changes);
    assert_eq!(*nix.built.lock().unwrap(), vec!["a".to_string()]);
    assert_eq!(&fixture.remote_head(), commit);
    let bare = &fixture.origin.bare;
    assert_eq!(
        git(bare, &["log", "-1", "--format=%an <%ae>|%s", commit]),
        "Lodestar <lodestar@vanwaaij.com>|chore(nixpkgs-unstable): bump for tailscale 1.98.10 -> 1.104.2"
    );
    assert_eq!(
        git(bare, &["rev-parse", &format!("{commit}^")]),
        fixture.base
    );
    assert_eq!(
        git(bare, &["show", &format!("{commit}:flake.lock")]),
        NEW_LOCK.trim()
    );
    let audits = update_audits(&fixture.state).await;
    assert_eq!(audits[0]["result"], "pushed");
    assert_eq!(audits[0]["commit"], commit.as_str());
    assert_eq!(audits[0]["changes"][0]["software"], "tailscale");
    assert_eq!(audits[0]["changes"][0]["from"], "1.98.10");
    assert_eq!(audits[0]["changes"][0]["to"], "1.104.2");
    assert!(!fixture.state_dir().join("updater").join(INPUT).exists());
}

#[tokio::test]
async fn changes_span_lowest_old_and_highest_new_across_hosts() {
    let mut fixture = fixture().await;
    let nix = install(
        &mut fixture,
        FakeNix {
            new_lock: Some(NEW_LOCK.into()),
            before: HashMap::from([
                ("a".into(), Ok(versions(&[("tailscale", "1.100.0")]))),
                ("b".into(), Ok(versions(&[("tailscale", "1.98.10")]))),
            ]),
            after: HashMap::from([
                ("a".into(), Ok(versions(&[("tailscale", "1.104.2")]))),
                ("b".into(), Ok(versions(&[("tailscale", "1.102.0")]))),
            ]),
            ..Default::default()
        },
    );
    let run = run_input(&fixture.state, INPUT, &software()).await;
    let UpdateOutcome::Pushed { changes, .. } = &run.outcome else {
        panic!("expected Pushed, got {:?}", run.outcome);
    };
    assert_eq!(
        changes,
        &vec![VersionChange {
            software: "tailscale".into(),
            from: "1.98.10".into(),
            to: "1.104.2".into(),
        }]
    );
    assert_eq!(
        *nix.built.lock().unwrap(),
        vec!["a".to_string(), "b".to_string()]
    );
}

#[tokio::test]
async fn failing_build_pushes_nothing() {
    let mut fixture = fixture().await;
    install(
        &mut fixture,
        FakeNix {
            new_lock: Some(NEW_LOCK.into()),
            before: before(),
            after: HashMap::from([
                ("a".into(), Ok(versions(&[("tailscale", "1.104.2")]))),
                ("b".into(), Ok(versions(&[("tailscale", "1.98.10")]))),
            ]),
            failing_builds: HashSet::from(["a".to_string()]),
            ..Default::default()
        },
    );
    let run = run_input(&fixture.state, INPUT, &software()).await;
    assert_eq!(
        run.outcome,
        UpdateOutcome::BuildFailed {
            host: "a".into(),
            log: "error: builder for a failed".into()
        }
    );
    assert_eq!(fixture.remote_head(), fixture.base);
    let audits = update_audits(&fixture.state).await;
    assert_eq!(audits[0]["result"], "build-failed");
    assert_eq!(audits[0]["host"], "a");
    assert_eq!(audits[0]["log"], "error: builder for a failed");
    assert!(!fixture.state_dir().join("updater").join(INPUT).exists());
}

#[tokio::test]
async fn failing_eval_pushes_nothing() {
    let mut fixture = fixture().await;
    let nix = install(
        &mut fixture,
        FakeNix {
            new_lock: Some(NEW_LOCK.into()),
            before: before(),
            after: HashMap::from([
                ("a".into(), Ok(versions(&[("tailscale", "1.104.2")]))),
                ("b".into(), Err("error: attribute missing".into())),
            ]),
            ..Default::default()
        },
    );
    let run = run_input(&fixture.state, INPUT, &software()).await;
    let UpdateOutcome::EvalFailed { host, log } = &run.outcome else {
        panic!("expected EvalFailed, got {:?}", run.outcome);
    };
    assert_eq!(host, "b");
    assert!(log.contains("attribute missing"), "{log}");
    assert!(nix.built.lock().unwrap().is_empty());
    assert_eq!(fixture.remote_head(), fixture.base);
    let audits = update_audits(&fixture.state).await;
    assert_eq!(audits[0]["result"], "eval-failed");
    assert_eq!(audits[0]["host"], "b");
}

#[tokio::test]
async fn missing_lock_after_the_update_stops_before_evaluating() {
    let mut fixture = fixture().await;
    let nix = install(
        &mut fixture,
        FakeNix {
            delete_lock: true,
            before: before(),
            after: HashMap::from([
                ("a".into(), Ok(versions(&[("tailscale", "1.104.2")]))),
                ("b".into(), Ok(versions(&[("tailscale", "1.104.2")]))),
            ]),
            ..Default::default()
        },
    );
    let run = run_input(&fixture.state, INPUT, &software()).await;
    let UpdateOutcome::EvalFailed { host, log } = &run.outcome else {
        panic!("expected EvalFailed, got {:?}", run.outcome);
    };
    assert_eq!(host, "");
    assert!(log.contains("flake.lock"), "{log}");
    assert_eq!(*nix.evals_after_update.lock().unwrap(), 0);
    assert!(nix.built.lock().unwrap().is_empty());
    assert_eq!(fixture.remote_head(), fixture.base);
    let audits = update_audits(&fixture.state).await;
    assert_eq!(audits[0]["result"], "eval-failed");
}

#[tokio::test]
async fn failing_eval_before_the_bump_stops_the_run() {
    let mut fixture = fixture().await;
    let nix = install(
        &mut fixture,
        FakeNix {
            new_lock: Some(NEW_LOCK.into()),
            before: HashMap::from([("a".into(), Err("error: broken".into()))]),
            ..Default::default()
        },
    );
    let run = run_input(&fixture.state, INPUT, &software()).await;
    assert!(
        matches!(&run.outcome, UpdateOutcome::EvalFailed { host, .. } if host == "a"),
        "{:?}",
        run.outcome
    );
    assert!(nix.updated.lock().unwrap().is_empty());
    assert_eq!(fixture.remote_head(), fixture.base);
}

#[tokio::test]
async fn invalid_host_name_is_eval_failed() {
    let mut fixture = fixture().await;
    fixture.state.hosts.push(host("bad host", "linux"));
    let nix = install(
        &mut fixture,
        FakeNix {
            new_lock: Some(NEW_LOCK.into()),
            before: before(),
            ..Default::default()
        },
    );
    let run = run_input(&fixture.state, INPUT, &software()).await;
    assert!(
        matches!(&run.outcome, UpdateOutcome::EvalFailed { host, .. } if host == "bad host"),
        "{:?}",
        run.outcome
    );
    assert!(nix.updated.lock().unwrap().is_empty());
}

#[tokio::test]
async fn moved_channel_is_push_failed_and_nothing_is_forced() {
    let mut fixture = fixture().await;
    let work = fixture.origin.work.clone();
    let other = Arc::new(Mutex::new(String::new()));
    let recorded = other.clone();
    install(
        &mut fixture,
        FakeNix {
            new_lock: Some(NEW_LOCK.into()),
            before: before(),
            after: HashMap::from([
                ("a".into(), Ok(versions(&[("tailscale", "1.104.2")]))),
                ("b".into(), Ok(versions(&[("tailscale", "1.98.10")]))),
            ]),
            on_build: Some(Box::new(move || {
                git(&work, &["commit", "--allow-empty", "-m", "someone else"]);
                git(&work, &["push", "origin", "HEAD:refs/heads/unstable"]);
                *recorded.lock().unwrap() = git(&work, &["rev-parse", "HEAD"]);
            })),
            ..Default::default()
        },
    );
    let run = run_input(&fixture.state, INPUT, &software()).await;
    let UpdateOutcome::PushFailed { reason } = &run.outcome else {
        panic!("expected PushFailed, got {:?}", run.outcome);
    };
    assert!(reason.contains("channel moved"), "{reason}");
    let other = other.lock().unwrap().clone();
    assert_eq!(fixture.remote_head(), other);
    assert_eq!(
        git(&fixture.origin.bare, &["rev-parse", &format!("{other}^")]),
        fixture.base
    );
    let audits = update_audits(&fixture.state).await;
    assert_eq!(audits[0]["result"], "push-failed");
    assert!(!fixture.state_dir().join("updater").join(INPUT).exists());
}

#[tokio::test]
async fn run_all_refuses_while_running() {
    let mut fixture = fixture().await;
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    install(
        &mut fixture,
        FakeNix {
            before: before(),
            gate: Some((entered.clone(), release.clone())),
            ..Default::default()
        },
    );
    let state = fixture.state.clone();
    let first = tokio::spawn(async move { run_all(&state).await });
    entered.notified().await;
    assert!(run_all(&fixture.state).await.is_err());
    release.notify_one();
    let runs = first.await.unwrap().expect("first run_all runs");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].input, INPUT);
    assert_eq!(runs[0].outcome, UpdateOutcome::NoChange);
    // Free again afterwards.
    let free = Arc::new(Notify::new());
    install(
        &mut fixture,
        FakeNix {
            before: before(),
            gate: Some((Arc::new(Notify::new()), free.clone())),
            ..Default::default()
        },
    );
    free.notify_one();
    assert!(run_all(&fixture.state).await.is_ok());
}

#[tokio::test]
async fn restore_last_reads_the_newest_audit_per_configured_input() {
    let mut fixture = fixture().await;
    install(
        &mut fixture,
        FakeNix {
            new_lock: Some(NEW_LOCK.into()),
            before: before(),
            after: HashMap::from([
                ("a".into(), Ok(versions(&[("tailscale", "1.104.2")]))),
                ("b".into(), Ok(versions(&[("tailscale", "1.98.10")]))),
            ]),
            ..Default::default()
        },
    );
    // An older run of this input and one of an input no longer configured.
    for input in [INPUT, "home-manager"] {
        db::insert_audit(
            &fixture.state.db,
            1,
            None,
            "update",
            "lodestar",
            serde_json::json!({"input": input, "result": "build-failed", "host": "b", "log": "boom"}),
        )
        .await
        .unwrap();
    }
    let pushed = run_input(&fixture.state, INPUT, &software()).await;
    assert!(matches!(pushed.outcome, UpdateOutcome::Pushed { .. }));
    fixture.state.updates.last.lock().unwrap().clear();

    restore_last(&fixture.state).await;
    let last = fixture.state.updates.last.lock().unwrap().clone();
    assert_eq!(last[INPUT], pushed);
    assert!(!last.contains_key("home-manager"));
    // The schedule after a restart starts from the restored run.
    assert_eq!(newest_finished(&fixture.state), Some(pushed.finished_at));
    assert_eq!(
        next_run_at(
            newest_finished(&fixture.state),
            pushed.finished_at + 60,
            86_400
        ),
        pushed.finished_at + 86_400
    );
}

#[test]
fn audit_details_round_trip() {
    let outcomes = [
        UpdateOutcome::Pushed {
            commit: "c".repeat(40),
            changes: vec![VersionChange {
                software: "tailscale".into(),
                from: "1.98.10".into(),
                to: "1.104.2".into(),
            }],
        },
        UpdateOutcome::NoChange,
        UpdateOutcome::EvalFailed {
            host: "a".into(),
            log: "e".into(),
        },
        UpdateOutcome::EvalFailed {
            host: String::new(),
            log: "no host".into(),
        },
        UpdateOutcome::BuildFailed {
            host: "b".into(),
            log: "f".into(),
        },
        UpdateOutcome::PushFailed {
            reason: "channel moved".into(),
        },
    ];
    for outcome in outcomes {
        let run = UpdateRun {
            input: INPUT.into(),
            finished_at: 42,
            outcome,
        };
        let details = run.details();
        assert_eq!(details["result"], run.outcome.result());
        assert_eq!(UpdateRun::from_details(42, &details), Some(run));
    }
    assert_eq!(
        UpdateRun::from_details(1, &serde_json::json!({"input": INPUT, "result": "other"})),
        None
    );
}

// ---- builds.enable: the updater builds through the gate ----

fn bumped(failing: &[&str]) -> FakeNix {
    FakeNix {
        new_lock: Some(NEW_LOCK.into()),
        before: before(),
        after: HashMap::from([
            ("a".into(), Ok(versions(&[("tailscale", "1.104.2")]))),
            ("b".into(), Ok(versions(&[("tailscale", "1.104.2")]))),
        ]),
        failing_builds: failing.iter().map(|name| name.to_string()).collect(),
        ..Default::default()
    }
}

/// Wraps the support fake; publishing can be made to fail.
struct PublishFails(support::FakeBuilder);

#[async_trait]
impl Builder for PublishFails {
    async fn build(&self, dir: &Path, hosts: &[ManagedHost]) -> BTreeMap<String, HostBuild> {
        self.0.build(dir, hosts).await
    }

    async fn publish(
        &self,
        _commit: &str,
        _paths: &[String],
        _channels: &serde_json::Value,
    ) -> anyhow::Result<()> {
        anyhow::bail!("cache unreachable")
    }
}

#[tokio::test]
async fn enabled_builds_changed_linux_hosts_through_the_gate_and_records_the_push() {
    let mut fixture = fixture_with(true).await;
    let nix = install(&mut fixture, bumped(&[]));
    let builder = Arc::new(support::FakeBuilder::default());
    fixture.state.builder = builder.clone();
    let run = run_input(&fixture.state, INPUT, &software()).await;
    let UpdateOutcome::Pushed { commit, .. } = &run.outcome else {
        panic!("expected Pushed, got {:?}", run.outcome);
    };
    // Linux through the gate in one call, darwin through the eval-only build.
    let calls = builder.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1, vec!["a".to_string()]);
    assert_eq!(*nix.built.lock().unwrap(), vec!["b".to_string()]);
    // The pushed commit has its result, so nothing builds it again.
    let result = fixture.state.builds.read(commit).expect("result recorded");
    assert!(result.hosts["a"].ok);
    assert!(!result.hosts.contains_key("b"));
    let hosts = fixture.state.hosts.clone();
    fixture
        .state
        .builds
        .ensure_built(&fixture.state, commit, &hosts)
        .await
        .unwrap();
    assert_eq!(builder.calls().len(), 1);
    let audits = update_audits(&fixture.state).await;
    assert!(audits[0].get("publish").is_none());
}

#[tokio::test]
async fn enabled_failed_linux_host_pushes_and_records_nothing() {
    let mut fixture = fixture_with(true).await;
    install(&mut fixture, bumped(&[]));
    fixture.state.builder = Arc::new(support::FakeBuilder::failing(&["a"]));
    let run = run_input(&fixture.state, INPUT, &software()).await;
    assert_eq!(
        run.outcome,
        UpdateOutcome::BuildFailed {
            host: "a".into(),
            log: "error: builder said no".into()
        }
    );
    assert_eq!(fixture.remote_head(), fixture.base);
    let builds = fixture.state_dir().join("builds");
    let recorded = std::fs::read_dir(&builds)
        .map(|entries| {
            entries
                .flatten()
                .any(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        })
        .unwrap_or(false);
    assert!(!recorded);
}

#[tokio::test]
async fn enabled_failed_darwin_eval_still_blocks_the_push() {
    let mut fixture = fixture_with(true).await;
    let nix = install(&mut fixture, bumped(&["b"]));
    fixture.state.builder = Arc::new(support::FakeBuilder::default());
    let run = run_input(&fixture.state, INPUT, &software()).await;
    assert!(matches!(
        run.outcome,
        UpdateOutcome::BuildFailed { ref host, .. } if host == "b"
    ));
    assert_eq!(*nix.built.lock().unwrap(), vec!["b".to_string()]);
    assert_eq!(fixture.remote_head(), fixture.base);
}

#[tokio::test]
async fn enabled_publish_failure_keeps_pushed_and_marks_the_audit() {
    let mut fixture = fixture_with(true).await;
    install(&mut fixture, bumped(&[]));
    fixture.state.builder = Arc::new(PublishFails(support::FakeBuilder::default()));
    let run = run_input(&fixture.state, INPUT, &software()).await;
    let UpdateOutcome::Pushed { commit, .. } = &run.outcome else {
        panic!("expected Pushed, got {:?}", run.outcome);
    };
    assert_eq!(&fixture.remote_head(), commit);
    let audits = update_audits(&fixture.state).await;
    assert_eq!(audits[0]["result"], "pushed");
    assert_eq!(audits[0]["publish"], "failed");
    // Nothing recorded: the next prebuild or apply builds and publishes it again.
    assert!(fixture.state.builds.read(commit).is_none());
}

#[tokio::test]
async fn disabled_never_touches_the_builder() {
    let mut fixture = fixture().await;
    let nix = install(&mut fixture, bumped(&[]));
    let builder = Arc::new(support::FakeBuilder::default());
    fixture.state.builder = builder.clone();
    let run = run_input(&fixture.state, INPUT, &software()).await;
    assert!(matches!(run.outcome, UpdateOutcome::Pushed { .. }));
    assert!(builder.calls().is_empty());
    assert_eq!(
        *nix.built.lock().unwrap(),
        vec!["a".to_string(), "b".to_string()]
    );
}
