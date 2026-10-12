mod common;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Method, Request, StatusCode};
use common::*;
use lodestar_agent::activations::read_since;
use lodestar_agent::api::{AppState, router};
use lodestar_agent::net::{SourceGuard, is_tailscale};
use lodestar_proto::{ActivationsResponse, AgentStatus, ApplyState};
use tower::ServiceExt;

const MANAGER: &str = "127.0.0.1:50000";

fn app(fixture: &Fixture, from: &str) -> Router {
    let config_path = fixture
        .config
        .state_dir
        .parent()
        .unwrap()
        .join("config.json");
    std::fs::write(&config_path, serde_json::to_vec(&fixture.config).unwrap()).unwrap();
    let state = AppState::new(
        fixture.config.clone(),
        config_path,
        SourceGuard::new("127.0.0.1".into()),
    )
    .unwrap();
    router(state).layer(MockConnectInfo(from.parse::<SocketAddr>().unwrap()))
}

fn configured() -> Fixture {
    let mut fixture = fixture();
    fixture.config.agent_exe = Some(PathBuf::from(env!("CARGO_BIN_EXE_lodestar-agent")));
    fixture.config.revision_command = Some(vec!["sh".into(), "-c".into(), "echo rev1".into()]);
    fixture.config.rebuild_command = Some(vec!["sh".into(), "-c".into(), "exit 0".into()]);
    fixture
}

async fn call(
    app: &Router,
    method: Method,
    uri: &str,
    body: Option<String>,
) -> (StatusCode, Vec<u8>) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(body.map(Body::from).unwrap_or_else(Body::empty))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    (
        status,
        to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
}

fn apply_body(id: &str, commit: &str) -> Option<String> {
    Some(format!(r#"{{"apply_id":"{id}","commit":"{commit}"}}"#))
}

async fn status(app: &Router) -> AgentStatus {
    let (code, body) = call(app, Method::GET, "/status", None).await;
    assert_eq!(code, StatusCode::OK);
    serde_json::from_slice(&body).unwrap()
}

async fn wait_state(app: &Router, want: ApplyState) -> AgentStatus {
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        let agent_status = status(app).await;
        if agent_status
            .apply
            .as_ref()
            .is_some_and(|apply| apply.state == want)
        {
            return agent_status;
        }
        assert!(
            Instant::now() < end,
            "timed out waiting for {want:?}: {agent_status:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn rejects_foreign_source() {
    let fixture = configured();
    let app = app(&fixture, "10.9.9.9:1234");
    let (code, _) = call(&app, Method::GET, "/status", None).await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    let (code, _) = call(
        &app,
        Method::POST,
        "/apply",
        apply_body("a1", &"0".repeat(40)),
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    let (code, _) = call(&app, Method::GET, "/activations?since=0", None).await;
    assert_eq!(code, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn bad_commit_400() {
    let fixture = configured();
    let app = app(&fixture, MANAGER);
    for commit in ["abc", &"A".repeat(40), &"g".repeat(40)] {
        let (code, _) = call(&app, Method::POST, "/apply", apply_body("a1", commit)).await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{commit}");
    }
    let (code, _) = call(
        &app,
        Method::POST,
        "/apply",
        apply_body("../x", &"0".repeat(40)),
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn apply_202_then_status_ok() {
    let fixture = configured();
    let commit = push_remote_commit(&fixture, "b.txt");
    lodestar_agent::git::Git::new(&fixture.config)
        .unwrap()
        .fetch(&fixture.config.repo_url)
        .unwrap();
    let app = app(&fixture, MANAGER);
    let (code, _) = call(&app, Method::POST, "/apply", apply_body("id-1", &commit)).await;
    assert_eq!(code, StatusCode::ACCEPTED);
    let agent_status = wait_state(&app, ApplyState::Ok).await;
    assert_eq!(agent_status.running, "rev1");
    assert_eq!(agent_status.host, "h");
    assert_eq!(agent_status.checkout.head.as_deref(), Some(commit.as_str()));
    assert_eq!(agent_status.checkout.branch.as_deref(), Some("unstable"));
    assert_eq!(agent_status.checkout.ahead, Some(0));
    let (code, body) = call(&app, Method::GET, "/apply/log?apply_id=id-1", None).await;
    assert_eq!(code, StatusCode::OK);
    let log: lodestar_proto::LogChunk = serde_json::from_slice(&body).unwrap();
    assert!(log.eof);
    let (code, _) = call(&app, Method::GET, "/apply/log?apply_id=nope&offset=0", None).await;
    assert_eq!(code, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn duplicate_apply_id_200() {
    let fixture = configured();
    let commit = push_remote_commit(&fixture, "b.txt");
    lodestar_agent::git::Git::new(&fixture.config)
        .unwrap()
        .fetch(&fixture.config.repo_url)
        .unwrap();
    let app = app(&fixture, MANAGER);
    let (code, _) = call(&app, Method::POST, "/apply", apply_body("id-1", &commit)).await;
    assert_eq!(code, StatusCode::ACCEPTED);
    wait_state(&app, ApplyState::Ok).await;
    let (code, body) = call(&app, Method::POST, "/apply", apply_body("id-1", &commit)).await;
    assert_eq!(code, StatusCode::OK);
    let record: lodestar_proto::ApplyRecord = serde_json::from_slice(&body).unwrap();
    assert_eq!(record.apply_id, "id-1");
    assert_eq!(record.state, ApplyState::Ok);
}

#[tokio::test]
async fn apply_while_running_409() {
    let mut fixture = configured();
    fixture.config.rebuild_command = Some(vec!["sh".into(), "-c".into(), "sleep 2".into()]);
    let commit = push_remote_commit(&fixture, "b.txt");
    lodestar_agent::git::Git::new(&fixture.config)
        .unwrap()
        .fetch(&fixture.config.repo_url)
        .unwrap();
    let app = app(&fixture, MANAGER);
    let (code, _) = call(&app, Method::POST, "/apply", apply_body("id-1", &commit)).await;
    assert_eq!(code, StatusCode::ACCEPTED);
    wait_state(&app, ApplyState::Running).await;
    let (code, _) = call(&app, Method::POST, "/apply", apply_body("id-2", &commit)).await;
    assert_eq!(code, StatusCode::CONFLICT);
    wait_state(&app, ApplyState::Ok).await;
}

#[tokio::test]
async fn body_over_4k_rejected() {
    let fixture = configured();
    let app = app(&fixture, MANAGER);
    let big = format!(r#"{{"apply_id":"{}","commit":"x"}}"#, "a".repeat(5000));
    let (code, _) = call(&app, Method::POST, "/apply", Some(big)).await;
    assert_eq!(code, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn activations_since_skips_partial_line() {
    let fixture = configured();
    std::fs::create_dir_all(&fixture.config.state_dir).unwrap();
    let path = fixture.config.state_dir.join("activations.jsonl");
    let activation_line = |time: i64, trigger: &str| {
        format!(
            r#"{{"time":{time},"store_path":"/nix/store/x","revision":"r{time}","generation":{time},"trigger":"{trigger}","sudo_user":null}}"#
        )
    };
    let content = format!(
        "{}\nnot json\n{}\n{{\"time\":3,\"sto",
        activation_line(1, "boot"),
        activation_line(2, "manual")
    );
    std::fs::write(&path, content).unwrap();

    let all = read_since(&path, 0);
    assert_eq!(all.next, 3);
    assert_eq!(all.events.len(), 2);
    assert_eq!(all.events[0].line, 0);
    assert_eq!(all.events[1].line, 2);
    assert_eq!(all.events[1].trigger, "manual");

    let rest = read_since(&path, 1);
    assert_eq!(rest.next, 3);
    assert_eq!(rest.events.len(), 1);

    // Once the partial line is completed it shows up with its own line number.
    let mut contents = std::fs::read_to_string(&path).unwrap();
    contents.truncate(contents.rfind("{\"time\":3").unwrap());
    contents.push_str(&format!("{}\n", activation_line(3, "apply:x")));
    std::fs::write(&path, contents).unwrap();
    let completed = read_since(&path, all.next);
    assert_eq!(completed.next, 4);
    assert_eq!(completed.events.len(), 1);
    assert_eq!(completed.events[0].line, 3);

    let app = app(&fixture, MANAGER);
    let (code, body) = call(&app, Method::GET, "/activations?since=2", None).await;
    assert_eq!(code, StatusCode::OK);
    let response: ActivationsResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(response.next, 4);
    assert_eq!(response.events.len(), 2);

    let missing = read_since(&fixture.config.state_dir.join("nope"), 0);
    assert!(missing.events.is_empty() && missing.next == 0);
}

#[test]
fn is_tailscale_ranges() {
    assert!(is_tailscale("100.95.164.9".parse().unwrap()));
    assert!(is_tailscale("100.64.0.1".parse().unwrap()));
    assert!(is_tailscale("100.127.255.255".parse().unwrap()));
    assert!(!is_tailscale("100.128.0.1".parse().unwrap()));
    assert!(!is_tailscale("100.63.0.1".parse().unwrap()));
    assert!(is_tailscale("fd7a:115c:a1e0::1".parse().unwrap()));
    assert!(!is_tailscale("fd7a:115c:a1e1::1".parse().unwrap()));
    assert!(!is_tailscale("10.0.0.1".parse().unwrap()));
}

#[tokio::test]
async fn back_to_back_applies_second_is_409() {
    let mut fixture = configured();
    fixture.config.rebuild_command = Some(vec!["sh".into(), "-c".into(), "sleep 1".into()]);
    let commit = push_remote_commit(&fixture, "b.txt");
    lodestar_agent::git::Git::new(&fixture.config)
        .unwrap()
        .fetch(&fixture.config.repo_url)
        .unwrap();
    let app = app(&fixture, MANAGER);
    let (first_code, _) = call(&app, Method::POST, "/apply", apply_body("id-1", &commit)).await;
    let (second_code, _) = call(&app, Method::POST, "/apply", apply_body("id-2", &commit)).await;
    let (third_code, _) = call(&app, Method::POST, "/apply", apply_body("id-1", &commit)).await;
    assert_eq!(
        (first_code, second_code, third_code),
        (
            StatusCode::ACCEPTED,
            StatusCode::CONFLICT,
            StatusCode::ACCEPTED
        )
    );
    wait_state(&app, ApplyState::Ok).await;
    let (code, _) = call(&app, Method::POST, "/apply", apply_body("id-1", &commit)).await;
    assert_eq!(code, StatusCode::OK);
}

#[test]
fn second_run_apply_exits_while_lock_held() {
    let fixture = configured();
    let lock = lodestar_agent::apply::try_lock_apply(&fixture.config).unwrap();
    let record = lodestar_agent::apply::run_apply(&fixture.config, "id-9", &"0".repeat(40));
    assert_eq!(record.state, ApplyState::Failed);
    assert_eq!(record.message.as_deref(), Some("another apply is running"));
    assert!(lodestar_agent::apply::read_record(&fixture.config, "id-9").is_none());
    assert!(
        !fixture
            .config
            .state_dir
            .join("apply-in-progress.json")
            .exists()
    );
    drop(lock);
    // A concurrently forking test thread can briefly inherit the descriptor
    // (O_CLOEXEC only applies at exec), so allow a moment for it to close.
    let end = Instant::now() + Duration::from_secs(5);
    while lodestar_agent::apply::try_lock_apply(&fixture.config).is_none() {
        assert!(Instant::now() < end, "lock never released");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[tokio::test]
async fn apply_records_last_fetch() {
    let fixture = configured();
    let commit = push_remote_commit(&fixture, "b.txt");
    let app = app(&fixture, MANAGER);
    assert!(status(&app).await.checkout.last_fetch_at.is_none());
    let (code, _) = call(&app, Method::POST, "/apply", apply_body("id-1", &commit)).await;
    assert_eq!(code, StatusCode::ACCEPTED);
    let agent_status = wait_state(&app, ApplyState::Ok).await;
    assert!(agent_status.checkout.last_fetch_at.is_some());
}
