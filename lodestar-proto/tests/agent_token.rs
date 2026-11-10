//! The bearer token the server presents to an agent: strength rules, token files, matching.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use lodestar_proto::{AgentToken, bearer_token};

const STRONG: &str = "3f9c1a7e5b2d4c6f8a0e1b3d5f7a9c2e4b6d8f0a1c3e5b7d9f2a4c6e8b0d1f3a";
const OTHER: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";

fn write(dir: &Path, name: &str, content: &str, mode: u32) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
    path
}

#[test]
fn strong_token_parses_and_matches_only_itself() {
    let token = AgentToken::parse(STRONG).unwrap();
    assert!(token.matches(STRONG));
    assert!(!token.matches(OTHER));
    assert!(!token.matches(""));
    assert!(!token.matches(&STRONG[..40]));
    assert!(!token.matches(&format!("{STRONG}x")));
}

#[test]
fn non_canonical_tokens_are_refused_without_echoing_them() {
    for weak in [
        "",
        "x1y2z",
        "changemechangemechangemechangeme",
        &format!("{} {}", &STRONG[..32], &STRONG[32..]),
        &format!("{}\u{e9}", &STRONG[..40]),
    ] {
        let error = AgentToken::parse(weak).unwrap_err().to_string();
        if weak.len() >= 4 {
            assert!(!error.contains(weak), "{error}");
        }
    }
}

#[test]
fn debug_never_shows_the_secret() {
    let token = AgentToken::parse(STRONG).unwrap();
    assert!(!format!("{token:?}").contains(&STRONG[..8]));
}

#[test]
fn authorization_header_round_trips() {
    let token = AgentToken::parse(STRONG).unwrap();
    let header = token.authorization();
    assert_eq!(header, format!("Bearer {STRONG}"));
    assert_eq!(bearer_token(&header), Some(STRONG));
    assert_eq!(bearer_token(&format!("bearer {STRONG}")), Some(STRONG));
    assert_eq!(bearer_token(&format!("Basic {STRONG}")), None);
    assert_eq!(bearer_token("Bearer"), None);
}

#[test]
fn token_file_holds_one_or_more_tokens_for_rotation() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(
        dir.path(),
        "token",
        &format!("{STRONG}\n\n{OTHER}\n"),
        0o400,
    );
    let tokens = AgentToken::read_file(&path).unwrap();
    assert_eq!(tokens.len(), 2);
    assert!(tokens.iter().any(|token| token.matches(OTHER)));
    assert!(
        AgentToken::read_one(&path).is_err(),
        "the server sends exactly one"
    );
    let single = write(dir.path(), "single", &format!("{STRONG}\n"), 0o600);
    assert!(AgentToken::read_one(&single).unwrap().matches(STRONG));
}

#[test]
fn token_file_refused_when_missing_empty_weak_or_readable_by_others() {
    let dir = tempfile::tempdir().unwrap();
    assert!(AgentToken::read_file(&dir.path().join("absent")).is_err());
    let empty = write(dir.path(), "empty", "\n", 0o400);
    assert!(AgentToken::read_file(&empty).is_err());
    let weak = write(dir.path(), "weak", &format!("{STRONG}\nshort\n"), 0o400);
    let error = AgentToken::read_file(&weak).unwrap_err();
    assert!(!format!("{error:#}").contains(STRONG));
    for mode in [0o440, 0o404, 0o644] {
        let open = write(dir.path(), &format!("open-{mode:o}"), STRONG, mode);
        let error = format!("{:#}", AgentToken::read_file(&open).unwrap_err());
        assert!(error.contains("group or others"), "{error}");
        assert!(!error.contains(STRONG));
    }
}

/// Only the format is checked (randomness cannot be): 32 bytes as 64 lowercase hex digits
/// (`openssl rand -hex 32`) or 43 unpadded base64url characters.
#[test]
fn only_a_canonical_encoding_of_32_bytes_is_accepted() {
    let base64url = "3_9c1a-e5b2d4c6f8a0e1b3d5f7a9c2e4b6d8f0a1cZ";
    assert_eq!(base64url.len(), 43);
    for accepted in [STRONG, OTHER, base64url] {
        assert!(AgentToken::parse(accepted).is_ok(), "{accepted}");
    }
    for refused in [
        // Predictable, varied text the old distinct-character rule let through.
        "0123456789".repeat(4),
        "0123456789abcdefghijABCDEFGHIJ0123456789".to_string(),
        STRONG[..62].to_string(),
        format!("{STRONG}ab"),
        STRONG.to_uppercase(),
        format!("{}+", &base64url[..42]),
        format!("{}/", &base64url[..42]),
        format!("{base64url}="),
        format!("{}g", &STRONG[..63]),
    ] {
        assert!(AgentToken::parse(&refused).is_err(), "{refused}");
    }
    // The limitation: well-formed but predictable bytes pass (like the VM tests' fixed
    // test-only token); generating them randomly is up to the operator.
    assert!(AgentToken::parse(&"0123456789abcdef".repeat(4)).is_ok());
}

#[test]
fn a_symlinked_token_file_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let target = write(dir.path(), "target", STRONG, 0o600);
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let error = format!("{:#}", AgentToken::read_file(&link).unwrap_err());
    assert!(!error.contains(STRONG), "{error}");
    assert!(AgentToken::read_file(&target).is_ok());
}

#[test]
fn a_token_path_that_is_not_a_regular_file_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let directory = dir.path().join("tokens");
    std::fs::create_dir(&directory).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    let error = format!("{:#}", AgentToken::read_file(&directory).unwrap_err());
    assert!(error.contains("regular file"), "{error}");
}

#[test]
fn a_token_file_in_a_directory_others_can_write_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let shared = dir.path().join("shared");
    std::fs::create_dir(&shared).unwrap();
    let path = write(&shared, "token", STRONG, 0o600);
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o777)).unwrap();
    let error = format!("{:#}", AgentToken::read_file(&path).unwrap_err());
    assert!(error.contains("directory"), "{error}");
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(AgentToken::read_file(&path).is_ok());
}
