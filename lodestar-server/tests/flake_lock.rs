//! What a channel's `flake.lock` may lock, and how far `nix flake update <input>` may move it.

use lodestar_server::flake_lock::{COMMITTED_LOCK, FlakeLock, RESTRICTED_EVALUATION};
use serde_json::{Value, json};

fn github(rev: &str) -> Value {
    json!({
        "locked": {
            "type": "github", "owner": "NixOS", "repo": "nixpkgs", "rev": rev,
            "narHash": "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=", "lastModified": 1,
        },
        "original": { "type": "github", "owner": "NixOS", "repo": "nixpkgs" },
    })
}

/// A lock whose root has `inputs` and whose other nodes are `nodes`.
fn lock(inputs: Value, nodes: &[(&str, Value)]) -> FlakeLock {
    let mut all = serde_json::Map::new();
    all.insert("root".into(), json!({ "inputs": inputs }));
    for (name, node) in nodes {
        all.insert((*name).into(), node.clone());
    }
    let text = json!({ "nodes": all, "root": "root", "version": 7 }).to_string();
    FlakeLock::parse(&text).unwrap()
}

/// A lock with a single input `x` locked as `locked` (and `original` as given).
fn single(locked: Value, original: Option<Value>) -> FlakeLock {
    let mut node = json!({ "flake": false, "locked": locked });
    if let Some(original) = original {
        node["original"] = original;
    }
    lock(json!({ "x": "x" }), &[("x", node)])
}

#[test]
fn remote_and_relative_sources_are_accepted() {
    let accepted = [
        json!({ "type": "github", "owner": "o", "repo": "r", "rev": "0" }),
        json!({ "type": "gitlab", "owner": "o", "repo": "r", "rev": "0" }),
        json!({ "type": "sourcehut", "owner": "~o", "repo": "r", "rev": "0" }),
        json!({ "type": "git", "url": "https://example.com/r.git", "rev": "0" }),
        json!({ "type": "tarball", "url": "https://example.com/r.tar.gz" }),
        json!({ "type": "file", "url": "https://example.com/r.json" }),
        json!({ "type": "path", "path": "./pinned" }),
        json!({ "type": "path", "path": "sub/dir" }),
    ];
    for locked in accepted {
        single(locked.clone(), None)
            .check_sources()
            .unwrap_or_else(|error| panic!("{locked} refused: {error:#}"));
    }
    // A registry name is what the lock was made from, not what is fetched.
    single(
        accepted_github(),
        Some(json!({ "type": "indirect", "id": "nixpkgs" })),
    )
    .check_sources()
    .unwrap();
}

fn accepted_github() -> Value {
    github("0")["locked"].clone()
}

#[test]
fn local_and_unknown_sources_are_refused() {
    let refused = [
        json!({ "type": "path", "path": "/var/lib/lodestar-server", "narHash": "sha256-x" }),
        json!({ "type": "path", "path": "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-source" }),
        json!({ "type": "path" }),
        json!({ "type": "git", "url": "file:///var/lib/lodestar-server/mirror", "rev": "0" }),
        json!({ "type": "git", "url": "/var/lib/lodestar-server/mirror", "rev": "0" }),
        json!({ "type": "git", "url": "http://example.com/r.git", "rev": "0" }),
        // Over ssh, git would authenticate with the server's own identities.
        json!({ "type": "git", "url": "ssh://git@example.com/r.git", "rev": "0" }),
        json!({ "type": "tarball", "url": "file:///run/keys/builder" }),
        json!({ "type": "tarball", "url": "http://127.0.0.1:8080/api/snapshot" }),
        json!({ "type": "file", "url": "file:///run/keys/builder" }),
        json!({ "type": "file", "url": "ssh://example.com/r" }),
        json!({ "type": "hg", "url": "https://example.com/r" }),
        json!({ "type": "indirect", "id": "nixpkgs" }),
        json!({ "type": "mystery" }),
        json!({}),
    ];
    for locked in refused {
        let error = single(locked.clone(), None)
            .check_sources()
            .expect_err(&format!("{locked} accepted"));
        assert!(error.to_string().contains("\"x\""), "{error:#}");
    }
}

#[test]
fn a_local_original_or_a_missing_lock_is_refused() {
    let local = json!({ "type": "path", "path": "/run/keys/builder" });
    assert!(
        single(accepted_github(), Some(local))
            .check_sources()
            .is_err()
    );
    let unlocked = lock(json!({ "x": "x" }), &[("x", json!({ "original": {} }))]);
    let error = unlocked.check_sources().unwrap_err();
    assert!(error.to_string().contains("not locked"), "{error:#}");
}

#[test]
fn malformed_locks_do_not_parse() {
    for text in [
        "",
        "not json",
        "{}",
        r#"{"nodes": [], "root": "root"}"#,
        r#"{"nodes": {}, "root": "root"}"#,
        r#"{"nodes": {"a": {}}, "root": "root"}"#,
    ] {
        assert!(FlakeLock::parse(text).is_err(), "{text:?} parsed");
    }
}

#[test]
fn no_lock_file_is_fine_and_a_symlinked_one_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    assert!(FlakeLock::read(dir.path()).unwrap().is_none());
    FlakeLock::check_dir(dir.path()).unwrap();

    // A symlink to a file only the server can read is never followed.
    let target = dir.path().join("private");
    std::fs::write(&target, "{\"nodes\":{\"root\":{}},\"root\":\"root\"}").unwrap();
    std::os::unix::fs::symlink(&target, dir.path().join("flake.lock")).unwrap();
    let error = FlakeLock::check_dir(dir.path()).unwrap_err();
    assert!(
        error.to_string().contains("not a regular file"),
        "{error:#}"
    );
}

#[test]
fn check_dir_reads_the_lock_in_the_directory() {
    let dir = tempfile::tempdir().unwrap();
    let text = json!({
        "nodes": {
            "root": { "inputs": { "x": "x" } },
            "x": { "locked": { "type": "path", "path": "/etc" } },
        },
        "root": "root",
        "version": 7,
    });
    std::fs::write(dir.path().join("flake.lock"), text.to_string()).unwrap();
    assert!(FlakeLock::check_dir(dir.path()).is_err());
}

fn before() -> FlakeLock {
    let mut home_manager = github("1");
    home_manager["inputs"] = json!({ "nixpkgs": ["nixpkgs"], "systems": "systems" });
    lock(
        json!({ "nixpkgs": "nixpkgs", "home-manager": "home-manager" }),
        &[
            ("nixpkgs", github("0")),
            ("home-manager", home_manager),
            ("systems", github("2")),
        ],
    )
}

#[test]
fn an_update_that_moves_only_its_input_is_bounded() {
    let mut home_manager = github("1");
    // Node names may change when Nix rewrites the lock.
    home_manager["inputs"] = json!({ "nixpkgs": ["nixpkgs"], "systems": "systems_2" });
    let mut nixpkgs = github("9");
    nixpkgs["inputs"] = json!({ "lib": "lib" });
    let after = lock(
        json!({ "nixpkgs": "nixpkgs", "home-manager": "home-manager" }),
        &[
            ("nixpkgs", nixpkgs),
            ("lib", github("8")),
            ("home-manager", home_manager),
            ("systems_2", github("2")),
        ],
    );
    before().check_update(&after, "nixpkgs").unwrap();
    before().check_update(&before(), "nixpkgs").unwrap();
}

#[test]
fn an_update_that_moves_another_input_is_refused() {
    // home-manager moved along with nixpkgs.
    let after = lock(
        json!({ "nixpkgs": "nixpkgs", "home-manager": "home-manager" }),
        &[
            ("nixpkgs", github("9")),
            (
                "home-manager",
                json!({
                    "locked": github("7")["locked"], "original": github("7")["original"],
                    "inputs": { "nixpkgs": ["nixpkgs"], "systems": "systems" },
                }),
            ),
            ("systems", github("2")),
        ],
    );
    let error = before().check_update(&after, "nixpkgs").unwrap_err();
    assert!(error.to_string().contains("\"home-manager\""), "{error:#}");

    // An input of another input moved.
    let mut home_manager = github("1");
    home_manager["inputs"] = json!({ "nixpkgs": ["nixpkgs"], "systems": "systems" });
    let after = lock(
        json!({ "nixpkgs": "nixpkgs", "home-manager": "home-manager" }),
        &[
            ("nixpkgs", github("9")),
            ("home-manager", home_manager.clone()),
            ("systems", github("6")),
        ],
    );
    assert!(before().check_update(&after, "nixpkgs").is_err());

    // A follows of another input changed.
    let mut followed = home_manager;
    followed["inputs"]["nixpkgs"] = json!(["systems"]);
    let after = lock(
        json!({ "nixpkgs": "nixpkgs", "home-manager": "home-manager" }),
        &[
            ("nixpkgs", github("0")),
            ("home-manager", followed),
            ("systems", github("2")),
        ],
    );
    assert!(before().check_update(&after, "nixpkgs").is_err());
}

#[test]
fn an_update_that_adds_or_drops_an_input_is_refused() {
    let mut home_manager = github("1");
    home_manager["inputs"] = json!({ "nixpkgs": ["nixpkgs"], "systems": "systems" });
    let added = lock(
        json!({ "nixpkgs": "nixpkgs", "home-manager": "home-manager", "secret": "secret" }),
        &[
            ("nixpkgs", github("0")),
            ("home-manager", home_manager),
            ("systems", github("2")),
            ("secret", github("5")),
        ],
    );
    let error = before().check_update(&added, "nixpkgs").unwrap_err();
    assert!(
        error.to_string().contains("changed which inputs"),
        "{error:#}"
    );

    let dropped = lock(json!({ "nixpkgs": "nixpkgs" }), &[("nixpkgs", github("0"))]);
    assert!(before().check_update(&dropped, "nixpkgs").is_err());
}

#[test]
fn a_cyclic_or_dangling_lock_does_not_hang() {
    let mut a = github("0");
    a["inputs"] = json!({ "b": "b" });
    let mut b = github("1");
    b["inputs"] = json!({ "a": "a" });
    let cyclic = lock(
        json!({ "x": "x", "a": "a" }),
        &[("x", github("3")), ("a", a), ("b", b)],
    );
    cyclic.check_update(&cyclic, "x").unwrap();

    let dangling = lock(json!({ "x": "x", "a": "missing" }), &[("x", github("3"))]);
    assert!(dangling.check_update(&dangling, "x").is_err());
}

#[test]
fn a_root_node_with_a_source_is_refused() {
    // An attacker names the exempted node (`root`) `"evil"` and then aliases it as an input of
    // their own node, so whatever `evil` locks would otherwise bypass `check_sources` (H-1).
    let text = json!({
        "version": 7,
        "root": "evil",
        "nodes": {
            "evil": {
                "inputs": { "leak": "evil" },
                "locked": { "type": "path", "path": "/run/secrets/lodestar-builder-key" },
                "original": { "type": "path", "path": "/run/secrets/lodestar-builder-key" },
            },
        },
    })
    .to_string();
    let error = FlakeLock::parse(&text).err().unwrap();
    assert!(
        error
            .to_string()
            .contains("root node must not have a source"),
        "{error:#}"
    );
}

#[test]
fn a_node_referring_to_the_root_by_name_is_refused() {
    // Even when the root carries no source itself (so it passes the check above), nothing
    // else may reference it by name as an input: Nix would then fetch it as a `LockedNode`,
    // using fields `check_sources` never looked at because it treats the root specially.
    let text = json!({
        "version": 7,
        "root": "root",
        "nodes": {
            "root": { "inputs": { "sneaky": "sneaky" } },
            "sneaky": { "inputs": { "leak": "root" } },
        },
    })
    .to_string();
    let lock = FlakeLock::parse(&text).unwrap();
    let error = lock.check_sources().unwrap_err();
    assert!(error.to_string().contains("\"sneaky\""), "{error:#}");
    assert!(error.to_string().contains("\"root\""), "{error:#}");
}

#[test]
fn a_relative_path_leaving_the_flake_is_refused() {
    let refused = [
        json!({ "type": "path", "path": "../.." }),
        json!({ "type": "path", "path": "../../../../etc/nix" }),
        json!({ "type": "path", "path": "sub/../../escape" }),
    ];
    for locked in refused {
        let error = single(locked.clone(), None)
            .check_sources()
            .expect_err(&format!("{locked} accepted"));
        assert!(error.to_string().contains("\"x\""), "{error:#}");
    }
}

#[test]
fn every_command_uses_the_committed_lock_and_restricted_evaluation() {
    assert_eq!(COMMITTED_LOCK, "--no-update-lock-file");
    let options = RESTRICTED_EVALUATION.join(" ");
    assert!(options.contains("--option restrict-eval true"), "{options}");
    assert!(
        options.contains("--option accept-flake-config false"),
        "{options}"
    );
    let allowed = RESTRICTED_EVALUATION
        .windows(2)
        .find(|pair| pair[0] == "allowed-uris")
        .map(|pair| pair[1])
        .unwrap();
    for scheme in allowed.split_whitespace() {
        assert!(
            ![
                "path:",
                "file:",
                "git+file:",
                "git+ssh:",
                "http:",
                "tarball+file:"
            ]
            .contains(&scheme),
            "{scheme}"
        );
    }
}
