//! The updater's `NixCommand` against a fake `nix` script (no real Nix): the options every
//! command runs with, what it refuses before running Nix, and how far `flake_update` may move
//! the lock.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use lodestar_server::config::ManagedHost;
use lodestar_server::flake_lock::RESTRICTED_EVALUATION;
use lodestar_server::nix_sandbox::NixSandbox;
use lodestar_server::updates::nix_runner::{NixCommand, NixRunner};
use serde_json::{Value, json};

fn host(name: &str, os: &str) -> ManagedHost {
    ManagedHost {
        name: name.into(),
        system: "x86_64-linux".into(),
        user: "alice".into(),
        channel: "unstable".into(),
        os: os.into(),
    }
}

/// The fake `nix`: records its arguments to `nix.calls`; `flake update` copies `new.lock` (next
/// to the script) over the worktree's `flake.lock`; `eval` prints an empty map.
const NIX: &str = r#"#!/bin/sh
dir=$(dirname "$0")
for argument in "$@"; do printf '%s\n' "$argument" >> "$dir/nix.calls"; done
echo >> "$dir/nix.calls"
shift 14
case "$1" in
  flake) cp "$dir/new.lock" flake.lock ;;
  eval) echo '{}' ;;
esac
"#;

struct Fixture {
    dir: tempfile::TempDir,
    runner: NixCommand,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("nix");
        std::fs::write(&bin, NIX).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::create_dir(dir.path().join("flake")).unwrap();
        let sandbox = NixSandbox::new(dir.path().join("sandbox")).also_allow(dir.path());
        Self {
            runner: NixCommand { bin, sandbox },
            dir,
        }
    }

    fn flake(&self) -> PathBuf {
        self.dir.path().join("flake")
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("nix.calls")).unwrap_or_default()
    }

    fn lock(&self) -> String {
        std::fs::read_to_string(self.flake().join("flake.lock")).unwrap()
    }

    fn set_lock(&self, lock: &Value) {
        write(&self.flake().join("flake.lock"), lock);
    }

    /// What the fake `nix flake update` writes.
    fn set_new_lock(&self, lock: &Value) {
        write(&self.dir.path().join("new.lock"), lock);
    }
}

fn write(path: &Path, lock: &Value) {
    std::fs::write(path, serde_json::to_string_pretty(lock).unwrap()).unwrap();
}

fn github(rev: &str) -> Value {
    json!({
        "locked": { "type": "github", "owner": "NixOS", "repo": "nixpkgs", "rev": rev },
        "original": { "type": "github", "owner": "NixOS", "repo": "nixpkgs" },
    })
}

/// A lock with root inputs `nixpkgs` and `other` locked at the given revisions, plus `extra`
/// root inputs.
fn lock(nixpkgs: &str, other: &str, extra: &[(&str, Value)]) -> Value {
    let mut lock = json!({
        "nodes": {
            "root": { "inputs": { "nixpkgs": "nixpkgs", "other": "other" } },
            "nixpkgs": github(nixpkgs),
            "other": github(other),
        },
        "root": "root",
        "version": 7,
    });
    for (name, node) in extra {
        lock["nodes"]["root"]["inputs"][*name] = json!(name);
        lock["nodes"][*name] = node.clone();
    }
    lock
}

#[tokio::test]
async fn every_command_uses_the_committed_lock_under_restricted_evaluation() {
    let fixture = Fixture::new();
    fixture.set_lock(&lock("0", "1", &[]));
    fixture.set_new_lock(&lock("2", "1", &[]));
    fixture
        .runner
        .eval_versions(&fixture.flake(), &host("a", "linux"))
        .await
        .unwrap();
    fixture
        .runner
        .build(&fixture.flake(), &host("a", "linux"))
        .await
        .unwrap();
    fixture
        .runner
        .build(&fixture.flake(), &host("m", "darwin"))
        .await
        .unwrap();
    fixture
        .runner
        .flake_update(&fixture.flake(), "nixpkgs")
        .await
        .unwrap();

    let calls = fixture.calls();
    let calls: Vec<Vec<&str>> = calls
        .split("\n\n")
        .filter(|call| !call.trim().is_empty())
        .map(|call| call.lines().collect())
        .collect();
    assert_eq!(calls.len(), 4);
    for call in &calls {
        assert_eq!(
            call[2..2 + RESTRICTED_EVALUATION.len()],
            *RESTRICTED_EVALUATION,
            "{call:?}"
        );
        assert!(!call.contains(&"--no-write-lock-file"), "{call:?}");
    }
    // Evaluations and builds never lock anything themselves.
    for call in &calls[..3] {
        assert!(call.contains(&"--no-update-lock-file"), "{call:?}");
    }
    assert_eq!(
        calls[3][2 + RESTRICTED_EVALUATION.len()..],
        ["flake", "update", "nixpkgs"]
    );
}

#[tokio::test]
async fn a_lock_with_a_local_source_is_refused_before_nix_runs() {
    let local = json!({
        "flake": false,
        "locked": { "type": "path", "path": "/var/lib/lodestar-server", "narHash": "sha256-x" },
    });
    let fixture = Fixture::new();
    fixture.set_lock(&lock("0", "1", &[("state", local)]));
    let error = fixture
        .runner
        .eval_versions(&fixture.flake(), &host("a", "linux"))
        .await
        .unwrap_err();
    assert!(format!("{error:#}").contains("\"state\""), "{error:#}");
    for os in ["linux", "darwin"] {
        let error = fixture
            .runner
            .build(&fixture.flake(), &host("a", os))
            .await
            .unwrap_err();
        assert!(error.contains("\"state\""), "{error}");
    }
    let error = fixture
        .runner
        .flake_update(&fixture.flake(), "nixpkgs")
        .await
        .unwrap_err();
    assert!(format!("{error:#}").contains("\"state\""), "{error:#}");
    assert_eq!(fixture.calls(), "");
}

#[tokio::test]
async fn an_update_that_moves_only_its_input_keeps_the_new_lock() {
    let fixture = Fixture::new();
    fixture.set_lock(&lock("0", "1", &[]));
    let new = lock("2", "1", &[]);
    fixture.set_new_lock(&new);
    fixture
        .runner
        .flake_update(&fixture.flake(), "nixpkgs")
        .await
        .unwrap();
    assert_eq!(fixture.lock(), serde_json::to_string_pretty(&new).unwrap());
}

#[tokio::test]
async fn an_update_that_moves_more_than_its_input_is_put_back() {
    let old = lock("0", "1", &[]);
    let cases = [
        ("another input moved", lock("2", "3", &[])),
        (
            "an input was added",
            lock("2", "1", &[("secret", github("4"))]),
        ),
        (
            "a local input was added",
            lock(
                "2",
                "1",
                &[(
                    "secret",
                    json!({ "locked": { "type": "path", "path": "/run/keys" } }),
                )],
            ),
        ),
        ("the lock is not JSON", json!("not a lock")),
    ];
    for (case, new) in cases {
        let fixture = Fixture::new();
        fixture.set_lock(&old);
        fixture.set_new_lock(&new);
        let result = fixture
            .runner
            .flake_update(&fixture.flake(), "nixpkgs")
            .await;
        assert!(result.is_err(), "{case}: accepted");
        assert_eq!(
            fixture.lock(),
            serde_json::to_string_pretty(&old).unwrap(),
            "{case}: the lock was not put back"
        );
    }
}

#[tokio::test]
async fn an_update_of_an_input_the_lock_lacks_never_runs_nix() {
    let fixture = Fixture::new();
    fixture.set_lock(&lock("0", "1", &[]));
    let error = fixture
        .runner
        .flake_update(&fixture.flake(), "missing")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("no input"), "{error:#}");
    assert_eq!(fixture.calls(), "");
}
