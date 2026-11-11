//! What a command confined by `NixSandbox` can read and write, checked with `sh` (no Nix):
//! the worktree yes, the rest of the state directory and the server's secrets no.

use std::path::{Path, PathBuf};

use lodestar_server::nix_sandbox::{Access, NixSandbox};
use tokio::process::Command;

struct Fixture {
    root: tempfile::TempDir,
    sandbox: NixSandbox,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        for dir in ["state", "worktree", "keys"] {
            std::fs::create_dir(root.path().join(dir)).unwrap();
        }
        std::fs::write(root.path().join("state/lodestar.db"), "sessions").unwrap();
        std::fs::write(root.path().join("keys/deploy"), "deploy key").unwrap();
        std::fs::write(root.path().join("keys/builder"), "builder key").unwrap();
        std::fs::write(root.path().join("worktree/flake.nix"), "{ }").unwrap();
        let sandbox = NixSandbox::new(root.path().join("state/nix-sandbox"));
        Self { root, sandbox }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    /// Runs `sh -c script` confined for `access`; whether it succeeded, and its stdout.
    async fn run(&self, script: &str, access: Access<'_>) -> (bool, String) {
        let mut command = Command::new(sh());
        command
            .arg("-c")
            .arg(script)
            .current_dir(self.path("worktree"));
        let _ruleset = self
            .sandbox
            .confine(&mut command, &self.path("worktree"), access)
            .unwrap();
        let output = command.output().await.unwrap();
        (
            output.status.success(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
        )
    }

    async fn can_read(&self, name: &str, access: Access<'_>) -> bool {
        let script = format!("cat '{}'", self.path(name).display());
        self.run(&script, access).await.0
    }
}

/// A shell from the Nix store (readable and executable when confined).
fn sh() -> PathBuf {
    let path = std::env::var_os("PATH").unwrap();
    std::env::split_paths(&path)
        .map(|dir| dir.join("sh"))
        .find(|candidate| candidate.is_file())
        .and_then(|sh| sh.canonicalize().ok())
        .filter(|sh| sh.starts_with("/nix/store"))
        .expect("an sh from the Nix store on PATH")
}

#[tokio::test]
async fn the_worktree_is_readable_and_the_secrets_are_not() {
    let fixture = Fixture::new();
    for access in [
        Access::Evaluate,
        Access::Update,
        Access::Builder { key: None },
    ] {
        assert!(fixture.can_read("worktree/flake.nix", access).await);
        for secret in ["state/lodestar.db", "keys/deploy", "keys/builder"] {
            assert!(
                !fixture.can_read(secret, access).await,
                "{secret} is readable for {access:?}"
            );
        }
    }
}

#[tokio::test]
async fn only_builder_commands_read_the_builder_key() {
    let fixture = Fixture::new();
    let key = fixture.path("keys/builder");
    assert!(
        fixture
            .can_read("keys/builder", Access::Builder { key: Some(&key) })
            .await
    );
    assert!(
        !fixture
            .can_read("keys/deploy", Access::Builder { key: Some(&key) })
            .await
    );
    assert!(!fixture.can_read("keys/builder", Access::Evaluate).await);
}

#[tokio::test]
async fn only_an_update_writes_the_worktree() {
    let fixture = Fixture::new();
    let write = "echo x > flake.lock";
    assert!(!fixture.run(write, Access::Evaluate).await.0);
    assert!(!fixture.path("worktree/flake.lock").exists());
    assert!(fixture.run(write, Access::Update).await.0);
    assert!(fixture.path("worktree/flake.lock").exists());
    // The state directory around the sandbox is never writable.
    let outside = format!("echo x > '{}'", fixture.path("state/planted").display());
    assert!(!fixture.run(&outside, Access::Update).await.0);
}

#[tokio::test]
async fn home_cache_and_tmp_are_private_to_the_sandbox() {
    let fixture = Fixture::new();
    let (ok, stdout) = fixture
        .run(
            "echo $HOME; echo $XDG_CACHE_HOME; echo $TMPDIR; touch \"$HOME/a\" \"$XDG_CACHE_HOME/b\" \"$TMPDIR/c\"",
            Access::Evaluate,
        )
        .await;
    assert!(ok, "{stdout}");
    let sandbox = fixture.path("state/nix-sandbox");
    for (line, name) in stdout.lines().zip(["home", "cache", "tmp"]) {
        assert_eq!(Path::new(line), sandbox.join(name));
        let mode = std::os::unix::fs::PermissionsExt::mode(
            &std::fs::metadata(sandbox.join(name)).unwrap().permissions(),
        );
        assert_eq!(mode & 0o777, 0o700, "{name}");
    }
}

#[tokio::test]
async fn a_linked_worktree_reads_its_git_directory() {
    let fixture = Fixture::new();
    let git_dir = fixture.path("state/mirror.git/worktrees/w");
    std::fs::create_dir_all(&git_dir).unwrap();
    std::fs::write(git_dir.join("HEAD"), "ref: refs/heads/unstable\n").unwrap();
    std::fs::write(git_dir.join("commondir"), "../..\n").unwrap();
    std::fs::write(fixture.path("state/mirror.git/config"), "[core]\n").unwrap();
    std::fs::write(
        fixture.path("worktree/.git"),
        format!("gitdir: {}\n", git_dir.display()),
    )
    .unwrap();
    assert!(
        fixture
            .can_read("state/mirror.git/worktrees/w/HEAD", Access::Evaluate)
            .await
    );
    assert!(
        fixture
            .can_read("state/mirror.git/config", Access::Evaluate)
            .await
    );
    assert!(
        !fixture
            .can_read("state/lodestar.db", Access::Evaluate)
            .await
    );
}
