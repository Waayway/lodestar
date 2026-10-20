//! The OpenID Connect client (PKCE, state, nonce, ES256) against a fake issuer.

mod support;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use lodestar_server::auth::{LoginError, LoginIdentity, LoginStart, OidcClient};
use lodestar_server::config::OidcConfig;
use serde_json::json;
use support::FakeIssuer;
use tempfile::TempDir;

const PUBLIC_URL: &str = "https://lodestar.example.com";
const READER_GROUP: &str = "lodestar-users@example.com";
const GROUP_UUID: &str = "3f1c5d6e-0a2b-4c8d-9e7f-123456789abc";

struct Setup {
    client: OidcClient,
    secret_dir: TempDir,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn setup(issuer: String) -> Setup {
    let secret_dir = TempDir::new().unwrap();
    let secret_file: PathBuf = secret_dir.path().join("secret");
    std::fs::write(&secret_file, "s3cret\n").unwrap();
    let config = OidcConfig {
        issuer,
        client_id: "lodestar".into(),
        client_secret_file: secret_file,
        reader_group: READER_GROUP.into(),
        admin_group: "admins@example.com".into(),
    };
    Setup {
        client: OidcClient::new(config, PUBLIC_URL).unwrap(),
        secret_dir,
    }
}

fn query(url: &str) -> HashMap<String, String> {
    reqwest::Url::parse(url)
        .unwrap()
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect()
}

/// `begin`, then the browser's visit to the authorize URL (the fake records the challenge).
async fn begin_and_authorize(client: &OidcClient) -> LoginStart {
    let start = client.begin("/".into(), now()).await.unwrap();
    let response = reqwest::get(&start.authorize_url).await.unwrap();
    assert!(response.status().is_success());
    start
}

fn refusal(result: Result<LoginIdentity, LoginError>) -> String {
    match result {
        Err(LoginError::Refused(reason)) => reason,
        other => panic!("expected Refused, got {other:?}"),
    }
}

#[tokio::test]
async fn begin_builds_pkce_url() {
    let fake = FakeIssuer::start().await;
    let setup = setup(fake.issuer());
    let start = setup
        .client
        .begin("/#/services".into(), now())
        .await
        .unwrap();

    assert!(
        start
            .authorize_url
            .starts_with(&format!("{}/authorize?", fake.issuer())),
        "{}",
        start.authorize_url
    );
    let params = query(&start.authorize_url);
    assert_eq!(params["response_type"], "code");
    assert_eq!(params["client_id"], "lodestar");
    assert_eq!(
        params["redirect_uri"],
        "https://lodestar.example.com/auth/callback"
    );
    let scopes: Vec<&str> = params["scope"].split(' ').collect();
    for scope in ["openid", "profile", "groups"] {
        assert!(scopes.contains(&scope), "{scopes:?}");
    }
    assert_eq!(params["code_challenge_method"], "S256");
    assert!(!params["code_challenge"].is_empty());
    assert_eq!(params["state"], start.state);
    assert_eq!(params["nonce"], start.pending.nonce);
    assert_eq!(start.pending.next, "/#/services");
    assert!(!start.pending.pkce_verifier.is_empty());
}

#[tokio::test]
async fn finish_accepts_es256_token() {
    let fake = FakeIssuer::start().await;
    let setup = setup(fake.issuer());
    let start = begin_and_authorize(&setup.client).await;
    fake.issue(
        "c1",
        json!({
            "sub": "alice-id",
            "preferred_username": "alice",
            "nonce": start.pending.nonce,
            "groups": [READER_GROUP, GROUP_UUID],
        }),
    );

    let identity = setup.client.finish("c1", start.pending).await.unwrap();

    assert_eq!(
        identity,
        LoginIdentity {
            user: "alice".into(),
            groups: vec![READER_GROUP.into(), GROUP_UUID.into()],
        }
    );
    assert_eq!(fake.last_client_secret().as_deref(), Some("s3cret"));
}

#[tokio::test]
async fn finish_refuses_wrong_nonce() {
    let fake = FakeIssuer::start().await;
    let setup = setup(fake.issuer());
    let start = begin_and_authorize(&setup.client).await;
    fake.issue(
        "c1",
        json!({"sub": "alice-id", "preferred_username": "alice", "nonce": "not-the-nonce"}),
    );

    let reason = refusal(setup.client.finish("c1", start.pending).await);
    assert!(reason.contains("nonce"), "{reason}");
}

#[tokio::test]
async fn finish_refuses_bad_signature() {
    let fake = FakeIssuer::start().await;
    let setup = setup(fake.issuer());
    let start = begin_and_authorize(&setup.client).await;
    fake.issue_signed_by_other_key(
        "c1",
        json!({"sub": "alice-id", "preferred_username": "alice", "nonce": start.pending.nonce}),
    );

    let reason = refusal(setup.client.finish("c1", start.pending).await);
    assert!(reason.contains("ignature"), "{reason}");
}

#[tokio::test]
async fn finish_refuses_unknown_code() {
    let fake = FakeIssuer::start().await;
    let setup = setup(fake.issuer());
    let start = begin_and_authorize(&setup.client).await;

    let reason = refusal(setup.client.finish("never-issued", start.pending).await);
    assert!(reason.contains("invalid_grant"), "{reason}");
}

#[tokio::test]
async fn finish_refuses_wrong_pkce_verifier() {
    let fake = FakeIssuer::start().await;
    let setup = setup(fake.issuer());
    let mut start = begin_and_authorize(&setup.client).await;
    fake.issue(
        "c1",
        json!({"sub": "alice-id", "preferred_username": "alice", "nonce": start.pending.nonce}),
    );
    start.pending.pkce_verifier = "a".repeat(43);

    let reason = refusal(setup.client.finish("c1", start.pending).await);
    assert!(reason.contains("invalid_grant"), "{reason}");
}

#[tokio::test]
async fn finish_refuses_missing_preferred_username() {
    let fake = FakeIssuer::start().await;
    let setup = setup(fake.issuer());
    let start = begin_and_authorize(&setup.client).await;
    fake.issue(
        "c1",
        json!({"sub": "alice-id", "nonce": start.pending.nonce, "groups": [READER_GROUP]}),
    );

    let reason = refusal(setup.client.finish("c1", start.pending).await);
    assert!(reason.contains("preferred_username"), "{reason}");
}

#[tokio::test]
async fn missing_groups_claim_gives_no_role() {
    let fake = FakeIssuer::start().await;
    let setup = setup(fake.issuer());
    let start = begin_and_authorize(&setup.client).await;
    fake.issue(
        "c1",
        json!({"sub": "carol-id", "preferred_username": "carol", "nonce": start.pending.nonce}),
    );

    let identity = setup.client.finish("c1", start.pending).await.unwrap();

    assert_eq!(identity.user, "carol");
    assert!(identity.groups.is_empty());
    let config = OidcConfig {
        issuer: fake.issuer(),
        client_id: "lodestar".into(),
        client_secret_file: PathBuf::from("/unused"),
        reader_group: READER_GROUP.into(),
        admin_group: "admins@example.com".into(),
    };
    assert_eq!(identity.role(&config), None);
}

#[tokio::test]
async fn unreadable_secret_is_misconfigured() {
    let fake = FakeIssuer::start().await;
    let setup = setup(fake.issuer());
    let start = begin_and_authorize(&setup.client).await;
    std::fs::remove_file(setup.secret_dir.path().join("secret")).unwrap();

    let result = setup.client.finish("c1", start.pending).await;
    assert!(
        matches!(result, Err(LoginError::Misconfigured(_))),
        "{result:?}"
    );
}

#[tokio::test]
async fn discovery_failure_is_issuer_unreachable() {
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = closed.local_addr().unwrap();
    drop(closed);
    let setup = setup(format!("http://{address}"));

    let result = setup.client.begin("/".into(), now()).await;
    assert!(
        matches!(result, Err(LoginError::IssuerUnreachable(_))),
        "{:?}",
        result.err()
    );
}

#[tokio::test]
async fn discovery_is_cached() {
    let fake = FakeIssuer::start().await;
    let setup = setup(fake.issuer());

    setup.client.begin("/".into(), now()).await.unwrap();
    setup.client.begin("/".into(), now()).await.unwrap();

    assert_eq!(fake.discovery_hits(), 1);
}
