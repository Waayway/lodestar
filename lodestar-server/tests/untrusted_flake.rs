//! Regression tests against the real `nix`: a writer of a channel branch controls the flake the
//! server evaluates (prebuilds, the build gate, the updater) before any apply. Whatever that flake
//! does, nothing readable only by the server (here a fake secret in a temporary directory) may
//! reach a build or evaluation log. Skipped, with a note, where no working `nix` is on PATH.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use lodestar_server::builds::{Builder, RemoteBuilder};
use lodestar_server::config::ManagedHost;
use lodestar_server::nix_sandbox::NixSandbox;
use lodestar_server::updates::nix_runner::{NixCommand, NixRunner};

/// The `nix` on PATH, when it runs.
fn real_nix() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let nix = std::env::split_paths(&path)
        .map(|dir| dir.join("nix"))
        .find(|candidate| candidate.is_file())?;
    let works = Command::new(&nix)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success());
    if works {
        Some(nix)
    } else {
        eprintln!("skipped: {} does not run", nix.display());
        None
    }
}

macro_rules! nix_or_skip {
    () => {
        match real_nix() {
            Some(nix) => nix,
            None => {
                eprintln!("skipped: no working nix on PATH");
                return;
            }
        }
    };
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

/// A server-only directory with a fresh fake secret, and the attacker's flake next to it.
struct Scene {
    root: tempfile::TempDir,
    secret: String,
}

impl Scene {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let secret = format!("lodestar-test-secret-{}-{nanos}", std::process::id());
        std::fs::create_dir(root.path().join("private")).unwrap();
        std::fs::write(root.path().join("private/token"), &secret).unwrap();
        std::fs::create_dir(root.path().join("flake")).unwrap();
        git(&root.path().join("flake"), &["init", "-q"]);
        Self { root, secret }
    }

    /// The confinement of the server's `nix` runs, under a state directory of its own.
    fn sandbox(&self) -> NixSandbox {
        NixSandbox::new(self.root.path().join("state/nix-sandbox"))
    }

    fn runner(&self, nix: &Path) -> NixCommand {
        NixCommand {
            bin: nix.to_path_buf(),
            sandbox: self.sandbox(),
        }
    }

    fn private(&self) -> PathBuf {
        self.root.path().join("private")
    }

    fn flake(&self) -> PathBuf {
        self.root.path().join("flake")
    }

    /// Writes (and `git add`s) `name` in the flake.
    fn write(&self, name: &str, text: &str) {
        let path = self.flake().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, text).unwrap();
        git(&self.flake(), &["add", "--", name]);
    }

    /// `flake.nix` whose every evaluated attribute throws `leak` (a Nix expression).
    fn write_flake(&self, inputs: &str, arguments: &str, leak: &str) {
        self.write(
            "flake.nix",
            &format!(
                r#"{{
  {inputs}
  outputs = {{ self{arguments} }}:
    let leak = throw ({leak});
    in {{
      nixosConfigurations.a.config = {{
        system.build.toplevel = leak;
        services.lodestar-agent = leak;
      }};
      darwinConfigurations.m.system = leak;
    }};
}}
"#
            ),
        );
    }

    /// The narHash of the private directory, as an attacker who learned it would lock it.
    fn private_nar_hash(&self, nix: &Path) -> String {
        let output = Command::new(nix)
            .args([
                "--extra-experimental-features",
                "nix-command",
                "hash",
                "path",
            ])
            .arg(self.private())
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    /// A flake input `secret` of the private directory, not in any lock.
    fn unlocked_local_input(&self) {
        let url = format!("path:{}", self.private().display());
        self.write_flake(
            &format!(r#"inputs.secret = {{ url = "{url}"; flake = false; }};"#),
            ", secret",
            r#"builtins.readFile "${secret}/token""#,
        );
    }

    /// The same input, committed to `flake.lock` with its real narHash.
    fn locked_local_input(&self, nix: &Path) {
        self.unlocked_local_input();
        let path = self.private().display().to_string();
        let lock = serde_json::json!({
            "nodes": {
                "root": { "inputs": { "secret": "secret" } },
                "secret": {
                    "flake": false,
                    "locked": {
                        "type": "path",
                        "path": path,
                        "narHash": self.private_nar_hash(nix),
                        "lastModified": 1,
                    },
                    "original": { "type": "path", "path": path },
                },
            },
            "root": "root",
            "version": 7,
        });
        self.write("flake.lock", &serde_json::to_string_pretty(&lock).unwrap());
    }

    /// No inputs: the private directory is fetched while evaluating, by its real narHash.
    fn evaluation_time_fetch(&self, nix: &Path) {
        let leak = format!(
            r#"builtins.readFile "${{builtins.fetchTree {{ type = "path"; path = "{}"; narHash = "{}"; }}}}/token""#,
            self.private().display(),
            self.private_nar_hash(nix),
        );
        self.write_flake("", "", &leak);
    }

    /// The same with `builtins.fetchurl` of a `file://` URL.
    fn evaluation_time_fetchurl(&self, nix: &Path) {
        let output = Command::new(nix)
            .args([
                "--extra-experimental-features",
                "nix-command",
                "hash",
                "file",
                "--type",
                "sha256",
            ])
            .arg(self.private().join("token"))
            .output()
            .unwrap();
        assert!(output.status.success());
        let hash = String::from_utf8(output.stdout).unwrap().trim().to_string();
        let leak = format!(
            r#"builtins.readFile (builtins.fetchurl {{ url = "file://{}"; sha256 = "{hash}"; }})"#,
            self.private().join("token").display(),
        );
        self.write_flake("", "", &leak);
    }

    /// Where Nix would put a copy of the private directory (a `path:` input of it).
    fn private_store_path(&self, nix: &Path) -> PathBuf {
        let bin = nix.parent().unwrap();
        let hash = Command::new(bin.join("nix-hash"))
            .args(["--type", "sha256", "--base32"])
            .arg(self.private())
            .output()
            .unwrap();
        assert!(hash.status.success());
        let hash = String::from_utf8(hash.stdout).unwrap();
        let path = Command::new(bin.join("nix-store"))
            .args(["--print-fixed-path", "--recursive", "sha256", hash.trim()])
            .arg("source")
            .output()
            .unwrap();
        assert!(path.status.success());
        PathBuf::from(String::from_utf8(path.stdout).unwrap().trim())
    }

    fn assert_not_copied(&self, nix: &Path) {
        let path = self.private_store_path(nix);
        assert!(
            !path.exists(),
            "the private directory was copied to the world-readable {}",
            path.display()
        );
    }

    fn assert_hidden(&self, what: &str, text: &str) {
        assert!(
            !text.contains(&self.secret),
            "{what} carries the private file's content"
        );
    }
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args([
            "-c",
            "user.name=alice",
            "-c",
            "user.email=alice@example.com",
        ])
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

fn builder(scene: &Scene, nix: PathBuf) -> RemoteBuilder {
    RemoteBuilder {
        nix_bin: nix,
        ssh_bin: PathBuf::from("/nonexistent/ssh"),
        // Never reached: every host fails at its local evaluation.
        store: "ssh-ng://alice@example.com".into(),
        ssh_key_file: None,
        publish_command: vec!["cache-publish".into()],
        timeout: Duration::from_secs(120),
        sandbox: scene.sandbox(),
    }
}

/// The builder's results and every way the updater's Nix reports on `scene`'s flake.
async fn every_log(scene: &Scene, nix: &Path) -> Vec<(String, String)> {
    let mut logs = Vec::new();
    let results = builder(scene, nix.to_path_buf())
        .build(&scene.flake(), &[host("a", "linux")])
        .await;
    let a = &results["a"];
    assert!(!a.ok, "the build of a must fail");
    logs.push(("builder".into(), a.log_tail.clone().unwrap_or_default()));

    let runner = scene.runner(nix);
    let versions = runner
        .eval_versions(&scene.flake(), &host("a", "linux"))
        .await;
    let error = versions.expect_err("evaluating the versions of a must fail");
    logs.push(("eval_versions".into(), format!("{error:#}")));
    let build = runner.build(&scene.flake(), &host("a", "linux")).await;
    logs.push(("build linux".into(), build.expect_err("must fail")));
    let darwin = runner.build(&scene.flake(), &host("m", "darwin")).await;
    logs.push(("build darwin".into(), darwin.expect_err("must fail")));
    logs
}

#[tokio::test]
async fn an_unlocked_local_input_is_never_locked_and_read() {
    let nix = nix_or_skip!();
    let scene = Scene::new();
    scene.unlocked_local_input();
    for (what, log) in every_log(&scene, &nix).await {
        scene.assert_hidden(&what, &log);
    }
    assert!(
        !scene.flake().join("flake.lock").exists(),
        "no lock file is written"
    );
}

#[tokio::test]
async fn a_committed_lock_of_a_local_input_is_refused() {
    let nix = nix_or_skip!();
    let scene = Scene::new();
    scene.locked_local_input(&nix);
    for (what, log) in every_log(&scene, &nix).await {
        scene.assert_hidden(&what, &log);
    }
}

#[tokio::test]
async fn an_evaluation_time_fetch_of_a_local_path_is_refused() {
    let nix = nix_or_skip!();
    let scene = Scene::new();
    scene.evaluation_time_fetch(&nix);
    for (what, log) in every_log(&scene, &nix).await {
        scene.assert_hidden(&what, &log);
    }
}

#[tokio::test]
async fn an_evaluation_time_fetchurl_of_a_local_file_is_refused() {
    let nix = nix_or_skip!();
    let scene = Scene::new();
    scene.evaluation_time_fetchurl(&nix);
    for (what, log) in every_log(&scene, &nix).await {
        scene.assert_hidden(&what, &log);
    }
}

/// The updater's `nix flake update <input>` may only move that input: an input the channel added
/// to `flake.nix` without locking it must not get locked (and then evaluated) along with it.
#[tokio::test]
async fn an_update_of_one_input_does_not_lock_another() {
    let nix = nix_or_skip!();
    let scene = Scene::new();
    // The channel as last locked: one relative input, `pinned`.
    scene.write("pinned/version", "1\n");
    scene.write(
        "flake.nix",
        r#"{
  inputs.pinned = { url = "path:./pinned"; flake = false; };
  outputs = { self, pinned }: { };
}
"#,
    );
    let status = Command::new(&nix)
        .args([
            "--extra-experimental-features",
            "nix-command flakes",
            "flake",
            "lock",
        ])
        .current_dir(scene.flake())
        .status()
        .unwrap();
    assert!(status.success());
    git(&scene.flake(), &["add", "flake.lock"]);
    let old_lock = std::fs::read(scene.flake().join("flake.lock")).unwrap();

    // The writer adds a local input next to it and leaves the lock alone.
    let url = format!("path:{}", scene.private().display());
    scene.write_flake(
        &format!(
            r#"inputs.pinned = {{ url = "path:./pinned"; flake = false; }};
  inputs.secret = {{ url = "{url}"; flake = false; }};"#
        ),
        ", pinned, secret",
        r#"builtins.readFile "${secret}/token""#,
    );

    let runner = scene.runner(&nix);
    let update = runner.flake_update(&scene.flake(), "pinned").await;
    let error = update.expect_err("an update that locks another input must fail");
    scene.assert_hidden("the update error", &format!("{error:#}"));
    assert_eq!(
        std::fs::read(scene.flake().join("flake.lock")).unwrap(),
        old_lock,
        "the lock is left as it was"
    );
    // Nix reads an input it locks before anything can refuse the lock: the private directory
    // must not be readable to it at all.
    scene.assert_not_copied(&nix);
    for (what, log) in every_log(&scene, &nix).await {
        scene.assert_hidden(&what, &log);
    }
}

/// Without `--no-write-lock-file` Nix refuses an unlocked input, but only after it read it to
/// lock it: by then a copy is in the world-readable store, for any local user to read.
#[tokio::test]
async fn an_unlocked_local_input_is_not_copied_to_the_store() {
    let nix = nix_or_skip!();
    let scene = Scene::new();
    scene.unlocked_local_input();
    every_log(&scene, &nix).await;
    scene.assert_not_copied(&nix);
}

/// An HTTP endpoint only the server reaches: counts connections, answers `body` to each.
struct Endpoint {
    port: u16,
    body: String,
    connections: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl Endpoint {
    fn start() -> Self {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let body = format!("internal-response-{port}-{}", std::process::id());
        let connections = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (count, answer) = (connections.clone(), body.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let mut request = [0u8; 4096];
                let _ = stream.read(&mut request);
                let response = format!(
                    "HTTP/1.0 200 OK\r\nContent-Length: {}\r\n\r\n{answer}",
                    answer.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        Self {
            port,
            body,
            connections,
        }
    }

    fn connections(&self) -> usize {
        self.connections.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// A fixed-output `builtin:fetchurl` derivation (here of an endpoint on the server's loopback)
/// imported while evaluating is built by the Nix daemon with the host's network, whatever
/// `allowed-uris` says. With the hash an earlier mismatch printed, its answer reaches the log.
#[tokio::test]
async fn evaluation_never_fetches_from_an_internal_endpoint() {
    let nix = nix_or_skip!();
    let scene = Scene::new();
    let endpoint = Endpoint::start();
    std::fs::write(scene.root.path().join("body"), &endpoint.body).unwrap();
    let hash = Command::new(&nix)
        .args([
            "--extra-experimental-features",
            "nix-command",
            "hash",
            "file",
            "--type",
            "sha256",
        ])
        .arg(scene.root.path().join("body"))
        .output()
        .unwrap();
    assert!(hash.status.success());
    let hash = String::from_utf8(hash.stdout).unwrap().trim().to_string();
    let leak = format!(
        r#"builtins.readFile (derivation {{
          name = "probe-{port}"; system = "builtin"; builder = "builtin:fetchurl";
          url = "http://127.0.0.1:{port}/data";
          outputHashMode = "flat"; outputHashAlgo = "sha256"; outputHash = "{hash}";
        }})"#,
        port = endpoint.port,
    );
    scene.write_flake("", "", &leak);
    for (what, log) in every_log(&scene, &nix).await {
        assert!(
            !log.contains(&endpoint.body),
            "{what} carries the endpoint's answer"
        );
    }
    assert_eq!(endpoint.connections(), 0, "the endpoint was reached");
}

/// What the server needs of a legitimate flake still works: a relative input, versions, a
/// darwin evaluation, a Linux build on the local daemon, an update of an input.
#[tokio::test]
async fn a_legitimate_flake_still_evaluates_builds_and_updates() {
    let nix = nix_or_skip!();
    let scene = Scene::new();
    scene.write("pinned/version", "1.2.3");
    scene.write(
        "flake.nix",
        r#"{
  inputs.pinned = { url = "path:./pinned"; flake = false; };
  outputs = { self, pinned }:
    let
      version = builtins.readFile "${pinned}/version";
      system = derivation {
        name = "system-${version}"; system = "x86_64-linux"; builder = "/bin/sh";
        args = [ "-c" "echo ${version} > $out" ];
      };
    in {
      nixosConfigurations.a.config = {
        system.build.toplevel = system;
        services.lodestar-agent.versions.tailscale = version;
      };
      darwinConfigurations.m.system = system;
    };
}
"#,
    );
    let status = Command::new(&nix)
        .args([
            "--extra-experimental-features",
            "nix-command flakes",
            "flake",
            "lock",
        ])
        .current_dir(scene.flake())
        .status()
        .unwrap();
    assert!(status.success());
    git(&scene.flake(), &["add", "flake.lock"]);

    let runner = scene.runner(&nix);
    let versions = runner
        .eval_versions(&scene.flake(), &host("a", "linux"))
        .await
        .unwrap();
    assert_eq!(versions["tailscale"], "1.2.3");
    runner
        .build(&scene.flake(), &host("m", "darwin"))
        .await
        .unwrap();
    runner
        .build(&scene.flake(), &host("a", "linux"))
        .await
        .unwrap();
    runner.flake_update(&scene.flake(), "pinned").await.unwrap();
}

/// Nix's own fetchers (in the confined client) never reach a plain-HTTP endpoint: `http:` is not
/// an allowed URI, so the request is refused before any connection, whatever the hash.
#[tokio::test]
async fn an_evaluation_time_fetch_of_an_http_endpoint_never_connects() {
    let nix = nix_or_skip!();
    let scene = Scene::new();
    let endpoint = Endpoint::start();
    let leak = format!(
        r#"builtins.readFile (builtins.fetchurl {{
          url = "http://127.0.0.1:{}/data";
          sha256 = "0000000000000000000000000000000000000000000000000000";
        }})"#,
        endpoint.port,
    );
    scene.write_flake("", "", &leak);
    for (what, log) in every_log(&scene, &nix).await {
        assert!(
            !log.contains(&endpoint.body),
            "{what} carries the endpoint's answer"
        );
    }
    assert_eq!(endpoint.connections(), 0, "the endpoint was reached");
}

/// A `github:` input is fetched (over HTTPS, with DNS and TLS roots) by the confined Nix: the
/// updater moves it from an old revision to the newest, then reads it. Needs the network.
#[tokio::test]
#[ignore = "needs the network (github.com)"]
async fn a_github_input_is_updated_and_read_when_confined() {
    let nix = nix_or_skip!();
    let scene = Scene::new();
    scene.write(
        "flake.nix",
        r#"{
  inputs.registry = { url = "github:NixOS/flake-registry"; flake = false; };
  outputs = { self, registry }: {
    nixosConfigurations.a.config.services.lodestar-agent.versions.registry =
      builtins.substring 0 1 (builtins.readFile "${registry}/flake-registry.json");
  };
}
"#,
    );
    // Locked (unconfined, as a writer would) at an old revision.
    let status = Command::new(&nix)
        .args([
            "--extra-experimental-features",
            "nix-command flakes",
            "flake",
            "lock",
            "--override-input",
            "registry",
            "github:NixOS/flake-registry/895a65f8d5acf848136ee8fe8e8f736f0d27df96",
        ])
        .current_dir(scene.flake())
        .status()
        .unwrap();
    assert!(status.success());
    git(&scene.flake(), &["add", "flake.lock"]);
    let old = std::fs::read_to_string(scene.flake().join("flake.lock")).unwrap();

    let runner = scene.runner(&nix);
    runner
        .flake_update(&scene.flake(), "registry")
        .await
        .unwrap();
    let new = std::fs::read_to_string(scene.flake().join("flake.lock")).unwrap();
    assert_ne!(old, new, "the input did not move");
    let versions = runner
        .eval_versions(&scene.flake(), &host("a", "linux"))
        .await
        .unwrap();
    assert_eq!(versions["registry"], "{");
}
