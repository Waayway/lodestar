//! The remote builder against fake `nix` and `ssh` scripts (no network, no real Nix).

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use lodestar_server::builds::{Builder, RemoteBuilder};
use lodestar_server::config::ManagedHost;

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const STORE: &str = "ssh-ng://alice@example.com:2222";
const PATH_A: &str = "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-nixos-system-a";
const PATH_B: &str = "/nix/store/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-nixos-system-b";
const PATH_C: &str = "/nix/store/cccccccccccccccccccccccccccccccc-nixos-system-c";

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
    dir: tempfile::TempDir,
    builder: RemoteBuilder,
}

impl Fixture {
    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// Every recorded invocation of the fake `program`, one argument per line, blank line
    /// between invocations; empty when it never ran.
    fn calls(&self, program: &str) -> String {
        std::fs::read_to_string(self.path(&format!("{program}.calls"))).unwrap_or_default()
    }

    fn flake(&self) -> PathBuf {
        self.path("flake")
    }
}

/// An executable `sh` script in `dir` that first records its arguments to `<name>.calls`.
fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    let calls = dir.join(format!("{name}.calls"));
    let text = format!(
        "#!/bin/sh\nfor argument in \"$@\"; do printf '%s\\n' \"$argument\" >> '{}'; done\necho >> '{}'\n{body}\n",
        calls.display(),
        calls.display(),
    );
    std::fs::write(&path, text).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// The fake `nix`: `eval` prints the outPath of hosts a, b and c and fails for `broken`;
/// `build` runs BUILD (like real Nix with a failed host: prints nothing, exits 1);
/// `path-info` knows a and c; `store info` runs STORE.
const NIX: &str = r#"
dir=$(dirname "$0")
case "$3" in
  eval)
    pwd > "$dir/eval.cwd"
    case "$*" in
      *'"a".config'*) printf '%s' /nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-nixos-system-a ;;
      *'"b".config'*) printf '%s' /nix/store/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-nixos-system-b ;;
      *'"c".config'*) printf '%s' /nix/store/cccccccccccccccccccccccccccccccc-nixos-system-c ;;
      *) echo "error: attribute 'broken' is broken" >&2; exit 1 ;;
    esac ;;
  build)
    pwd > "$dir/build.cwd"
    printf '%s' "$NIX_SSHOPTS" > "$dir/build.sshopts"
    BUILD ;;
  path-info)
    case "$6" in
      *aaaaaaaa*|*cccccccc*) printf '%s\n' "$6" ;;
      *) echo "error: path '$6' is not valid" >&2; exit 1 ;;
    esac ;;
  store)
    STORE ;;
esac
"#;

/// A failed build of b: 50 log lines, nothing printed, exit 1.
const BUILD_FAILS: &str =
    r#"i=1; while [ $i -le 50 ]; do echo "log line $i" >&2; i=$((i+1)); done; exit 1"#;
const STORE_ANSWERS: &str = "echo 'Store URL: ssh-ng://alice@example.com'";

fn nix(build: &str, store: &str) -> String {
    NIX.replace("BUILD", build).replace("STORE", store)
}

/// The fake `ssh`: stores its stdin.
const SSH: &str = r#"
cat > "$(dirname "$0")/ssh.stdin"
"#;

fn fixture_with(nix: &str, ssh: &str, timeout: Duration) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("flake")).unwrap();
    let builder = RemoteBuilder {
        nix_bin: script(dir.path(), "nix", nix),
        ssh_bin: script(dir.path(), "ssh", ssh),
        store: STORE.into(),
        ssh_key_file: Some(PathBuf::from("/run/keys/builder")),
        publish_command: vec!["cache-publish".into()],
        timeout,
    };
    Fixture { dir, builder }
}

fn fixture() -> Fixture {
    fixture_with(
        &nix(BUILD_FAILS, STORE_ANSWERS),
        SSH,
        Duration::from_secs(30),
    )
}

/// The recorded invocations of `nix` whose subcommand is `subcommand`.
fn nix_calls(fixture: &Fixture, subcommand: &str) -> Vec<String> {
    fixture
        .calls("nix")
        .split("\n\n")
        .filter(|call| call.lines().nth(2) == Some(subcommand))
        .map(str::to_string)
        .collect()
}

#[tokio::test]
async fn built_hosts_are_found_on_the_store_and_failed_ones_get_the_log_tail() {
    let fixture = fixture();
    let results = fixture
        .builder
        .build(&fixture.flake(), &[host("a", "linux"), host("b", "linux")])
        .await;

    let a = &results["a"];
    assert!(a.ok);
    assert_eq!(a.path.as_deref(), Some(PATH_A));
    assert_eq!(a.log_tail, None);

    let b = &results["b"];
    assert!(!b.ok);
    assert_eq!(b.path, None);
    let tail = b.log_tail.as_deref().unwrap();
    assert_eq!(tail.lines().count(), 40);
    assert!(tail.starts_with("log line 11\n"), "{tail}");
    assert!(tail.ends_with("log line 50"), "{tail}");

    let calls = fixture.calls("nix");
    let build: Vec<&str> = calls
        .split("\n\n")
        .find(|call| call.contains("\nbuild\n"))
        .unwrap()
        .lines()
        .collect();
    assert_eq!(
        build,
        [
            "--extra-experimental-features",
            "nix-command flakes",
            "build",
            "--no-link",
            "--print-out-paths",
            "--keep-going",
            "--no-write-lock-file",
            "--store",
            STORE,
            "--eval-store",
            "auto",
            ".#nixosConfigurations.\"a\".config.system.build.toplevel",
            ".#nixosConfigurations.\"b\".config.system.build.toplevel",
        ]
    );
    assert_eq!(
        std::fs::read_to_string(fixture.path("build.sshopts")).unwrap(),
        "-i /run/keys/builder -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new -o BatchMode=yes -o ConnectTimeout=10"
    );
    // Nix exited 1: the store was asked once whether it answers, then about each path.
    assert_eq!(nix_calls(&fixture, "store").len(), 1);
    assert_eq!(nix_calls(&fixture, "path-info").len(), 2);
    let flake = fixture.flake().canonicalize().unwrap();
    for cwd in ["eval.cwd", "build.cwd"] {
        let recorded = std::fs::read_to_string(fixture.path(cwd)).unwrap();
        assert_eq!(Path::new(recorded.trim()).canonicalize().unwrap(), flake);
    }
}

#[tokio::test]
async fn a_path_already_on_the_builder_store_is_ok_although_not_printed() {
    let fixture = fixture();
    let results = fixture
        .builder
        .build(&fixture.flake(), &[host("c", "linux")])
        .await;
    assert!(results["c"].ok);
    assert_eq!(results["c"].path.as_deref(), Some(PATH_C));
}

#[tokio::test]
async fn an_eval_failure_has_its_own_tail_and_is_not_built() {
    let fixture = fixture();
    let results = fixture
        .builder
        .build(
            &fixture.flake(),
            &[host("a", "linux"), host("broken", "linux")],
        )
        .await;
    assert!(results["a"].ok);
    let broken = &results["broken"];
    assert!(!broken.ok);
    assert_eq!(
        broken.log_tail.as_deref(),
        Some("error: attribute 'broken' is broken")
    );
    let calls = fixture.calls("nix");
    let build = calls
        .split("\n\n")
        .find(|call| call.contains("\nbuild\n"))
        .unwrap();
    assert!(!build.contains("broken"), "{build}");
}

#[tokio::test]
async fn invalid_and_darwin_hosts_fail_before_any_process_runs() {
    let fixture = fixture();
    let results = fixture
        .builder
        .build(
            &fixture.flake(),
            &[
                host("bad name", "linux"),
                host("a/b", "linux"),
                host("mac", "darwin"),
            ],
        )
        .await;
    assert_eq!(results.len(), 3);
    assert!(results.values().all(|result| !result.ok));
    assert!(
        results["bad name"]
            .log_tail
            .as_deref()
            .unwrap()
            .contains("invalid host name")
    );
    assert_eq!(
        results["mac"].log_tail.as_deref(),
        Some("darwin is not built here")
    );
    assert_eq!(fixture.calls("nix"), "");
}

#[tokio::test]
async fn connection_errors_in_a_build_log_do_not_make_the_builder_unreachable() {
    let build = r#"echo "test_db: connect to 127.0.0.1 port 5432: Connection refused" >&2; echo "Could not resolve host: example.com" >&2; exit 1"#;
    let fixture = fixture_with(&nix(build, STORE_ANSWERS), SSH, Duration::from_secs(30));
    let results = fixture
        .builder
        .build(&fixture.flake(), &[host("a", "linux"), host("b", "linux")])
        .await;
    assert!(results["a"].ok);
    assert_eq!(results["a"].path.as_deref(), Some(PATH_A));
    let tail = results["b"].log_tail.as_deref().unwrap();
    assert!(!results["b"].ok);
    assert!(!tail.starts_with("builder unreachable"), "{tail}");
    assert!(tail.contains("Connection refused"), "{tail}");
}

#[tokio::test]
async fn a_store_that_does_not_answer_makes_every_host_unreachable() {
    let store = r#"echo "error: cannot connect to 'alice@example.com'" >&2; exit 1"#;
    let fixture = fixture_with(&nix(BUILD_FAILS, store), SSH, Duration::from_secs(30));
    let results = fixture
        .builder
        .build(&fixture.flake(), &[host("a", "linux"), host("b", "linux")])
        .await;
    for name in ["a", "b"] {
        assert!(!results[name].ok);
        assert_eq!(
            results[name].log_tail.as_deref(),
            Some("builder unreachable: error: cannot connect to 'alice@example.com'")
        );
    }
    let probe = nix_calls(&fixture, "store");
    assert_eq!(probe.len(), 1);
    assert_eq!(
        probe[0].lines().collect::<Vec<_>>(),
        [
            "--extra-experimental-features",
            "nix-command flakes",
            "store",
            "info",
            "--store",
            STORE
        ]
    );
    assert!(nix_calls(&fixture, "path-info").is_empty());
}

#[tokio::test]
async fn a_successful_build_needs_no_probe() {
    let build = r#"echo /nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-nixos-system-a"#;
    let fixture = fixture_with(&nix(build, "exit 1"), SSH, Duration::from_secs(30));
    let results = fixture
        .builder
        .build(&fixture.flake(), &[host("a", "linux")])
        .await;
    assert!(results["a"].ok);
    assert!(nix_calls(&fixture, "store").is_empty());
    assert!(nix_calls(&fixture, "path-info").is_empty());
}

#[tokio::test]
async fn a_build_past_the_timeout_is_killed() {
    let nix = r#"
echo $$ > "$(dirname "$0")/nix.pid"
exec sleep 30
"#;
    let fixture = fixture_with(nix, SSH, Duration::from_millis(500));
    let started = std::time::Instant::now();
    let results = fixture
        .builder
        .build(&fixture.flake(), &[host("a", "linux")])
        .await;
    assert!(started.elapsed() < Duration::from_secs(10));
    let tail = results["a"].log_tail.as_deref().unwrap();
    assert!(!results["a"].ok);
    assert!(tail.contains("timed out"), "{tail}");

    let pid = std::fs::read_to_string(fixture.path("nix.pid")).unwrap();
    let pid = pid.trim();
    // Killed: gone, or a zombie waiting to be reaped.
    let mut alive = true;
    for _ in 0..50 {
        let output = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", pid])
            .output()
            .unwrap();
        let stat = String::from_utf8_lossy(&output.stdout);
        if !output.status.success() || stat.trim().starts_with('Z') {
            alive = false;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(!alive, "nix ({pid}) still runs");
}

#[tokio::test]
async fn publish_runs_the_command_over_ssh_with_the_channels_on_stdin() {
    let fixture = fixture();
    let channels = serde_json::json!({ "stable": { "current": COMMIT, "previous": null } });
    fixture
        .builder
        .publish(COMMIT, &[PATH_A.into(), PATH_B.into()], &channels)
        .await
        .unwrap();
    let calls = fixture.calls("ssh");
    let arguments: Vec<&str> = calls.trim_end().lines().collect();
    assert_eq!(
        arguments,
        [
            "-i",
            "/run/keys/builder",
            "-o",
            "IdentitiesOnly=yes",
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            "-p",
            "2222",
            "--",
            "alice@example.com",
            "cache-publish",
            COMMIT,
            PATH_A,
            PATH_B,
        ]
    );
    let stdin = std::fs::read_to_string(fixture.path("ssh.stdin")).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&stdin).unwrap(),
        channels
    );
}

#[tokio::test]
async fn publish_rejects_bad_arguments_before_running_ssh() {
    let fixture = fixture();
    let channels = serde_json::json!({});
    for (commit, path) in [
        ("not-a-commit", PATH_A),
        (COMMIT, "/etc/passwd"),
        (COMMIT, "/nix/store/x; rm -rf /"),
        (COMMIT, "/nix/store/../../etc"),
        (COMMIT, "/nix/store/short-name"),
        (COMMIT, "/nix/store/eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee-name"),
        (COMMIT, "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-a?b"),
        (
            COMMIT,
            "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-.hidden",
        ),
    ] {
        let result = fixture
            .builder
            .publish(commit, &[path.into()], &channels)
            .await;
        assert!(result.is_err(), "{commit} {path}");
    }
    assert_eq!(fixture.calls("ssh"), "");
}

#[tokio::test]
async fn publish_with_ssh_unable_to_connect_is_builder_unreachable() {
    let ssh = r#"
echo "ssh: connect to host example.com port 2222: Connection refused" >&2
exit 255
"#;
    let fixture = fixture_with(
        &nix(BUILD_FAILS, STORE_ANSWERS),
        ssh,
        Duration::from_secs(30),
    );
    let error = fixture
        .builder
        .publish(COMMIT, &[PATH_A.into()], &serde_json::json!({}))
        .await
        .unwrap_err();
    let message = format!("{error:#}");
    assert!(message.starts_with("builder unreachable: "), "{message}");
}

#[tokio::test]
async fn publish_failure_carries_the_log_tail() {
    let ssh = r#"
echo "cache-publish: disk full" >&2
exit 3
"#;
    let fixture = fixture_with(
        &nix(BUILD_FAILS, STORE_ANSWERS),
        ssh,
        Duration::from_secs(30),
    );
    let error = fixture
        .builder
        .publish(COMMIT, &[PATH_A.into()], &serde_json::json!({}))
        .await
        .unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("cache-publish: disk full"), "{message}");
    assert!(!message.starts_with("builder unreachable"), "{message}");
}
