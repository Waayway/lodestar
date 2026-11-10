//! The server's agent client authenticates every call with the host's token, never sends
//! it anywhere else and never puts it in an error.

mod support;

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use lodestar_proto::{ActivationsResponse, AgentStatus, ApplyRequest, CheckoutStatus, LogChunk};
use lodestar_server::agent_client::{AgentClient, AgentEndpoint};
use support::{AGENT_TOKEN, agent_token_files, require_agent_token};

const OTHER_TOKEN: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";

/// The `Authorization` header of every request a listener saw (`None` when absent).
type Seen = Arc<Mutex<Vec<Option<String>>>>;

async fn record(State(seen): State<Seen>, request: Request, next: Next) -> Response {
    let authorization = request
        .headers()
        .get(header::AUTHORIZATION)
        .map(|value| value.to_str().unwrap().to_string());
    seen.lock().unwrap().push(authorization);
    next.run(request).await
}

async fn serve(app: Router) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

fn agent_status() -> AgentStatus {
    AgentStatus {
        host: "box".into(),
        os: "linux".into(),
        agent_version: "0".into(),
        running: "rev1".into(),
        checkout: CheckoutStatus {
            branch: None,
            head: None,
            dirty: false,
            untracked: 0,
            ahead: None,
            last_fetch_at: None,
            flake_path: None,
        },
        apply: None,
        versions: Default::default(),
    }
}

/// A fake agent that demands `AGENT_TOKEN`, recording what it was sent.
async fn fake_agent(seen: Seen) -> SocketAddr {
    let app = Router::new()
        .route("/status", get(|| async { axum::Json(agent_status()) }))
        .route("/apply", post(|| async { StatusCode::ACCEPTED }))
        .route(
            "/apply/log",
            get(|| async {
                axum::Json(LogChunk {
                    apply_id: "a1".into(),
                    offset: 0,
                    data: String::new(),
                    next: 0,
                    eof: true,
                })
            }),
        )
        .route(
            "/activations",
            get(|| async {
                axum::Json(ActivationsResponse {
                    events: vec![],
                    next: 0,
                })
            }),
        )
        .layer(middleware::from_fn(require_agent_token))
        .layer(middleware::from_fn_with_state(seen, record));
    serve(app).await
}

fn endpoint(host: &str, addr: SocketAddr) -> AgentEndpoint {
    AgentEndpoint {
        host: host.into(),
        addr: addr.to_string(),
    }
}

fn apply_request() -> ApplyRequest {
    ApplyRequest {
        apply_id: "a1".into(),
        commit: "0".repeat(40),
    }
}

#[tokio::test]
async fn every_call_carries_the_hosts_token() {
    let dir = tempfile::tempdir().unwrap();
    let client = AgentClient::new(agent_token_files(dir.path(), ["box"]), ["box"]).unwrap();
    let seen = Seen::default();
    let agent = endpoint("box", fake_agent(seen.clone()).await);

    client.status(&agent).await.unwrap();
    assert_eq!(
        client.apply(&agent, &apply_request()).await.unwrap(),
        StatusCode::ACCEPTED
    );
    client.log(&agent, "a1", 0).await.unwrap();
    client.activations(&agent, 0).await.unwrap();

    let expected = Some(format!("Bearer {AGENT_TOKEN}"));
    assert_eq!(*seen.lock().unwrap(), vec![expected; 4]);
}

#[tokio::test]
async fn each_host_gets_its_own_token() {
    let dir = tempfile::tempdir().unwrap();
    let mut files = agent_token_files(dir.path(), ["box"]);
    let other = dir.path().join("other-token");
    std::fs::write(&other, OTHER_TOKEN).unwrap();
    std::fs::set_permissions(&other, std::fs::Permissions::from_mode(0o400)).unwrap();
    files.insert("laptop".into(), other);
    let client = AgentClient::new(files, ["box", "laptop"]).unwrap();
    let seen = Seen::default();
    let addr = fake_agent(seen.clone()).await;

    // The fake accepts only box's token: laptop's token does not open box's agent.
    assert!(client.status(&endpoint("laptop", addr)).await.is_err());
    assert_eq!(
        *seen.lock().unwrap(),
        vec![Some(format!("Bearer {OTHER_TOKEN}"))]
    );
}

#[test]
fn every_managed_host_needs_a_token_file() {
    let dir = tempfile::tempdir().unwrap();
    let files = agent_token_files(dir.path(), ["box"]);
    let error = AgentClient::new(files, ["box", "laptop"]).err().unwrap();
    assert!(format!("{error:#}").contains("laptop"), "{error:#}");
}

#[tokio::test]
async fn an_unusable_token_file_fails_the_call_before_contacting_the_agent() {
    let dir = tempfile::tempdir().unwrap();
    let files = agent_token_files(dir.path(), ["box"]);
    let path = files["box"].clone();
    let client = AgentClient::new(files, ["box"]).unwrap();
    let seen = Seen::default();
    let agent = endpoint("box", fake_agent(seen.clone()).await);

    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let error = format!("{:#}", client.status(&agent).await.unwrap_err());
    assert!(error.contains("box"), "{error}");
    assert!(!error.contains(AGENT_TOKEN), "{error}");
    std::fs::remove_file(&path).unwrap();
    assert!(client.apply(&agent, &apply_request()).await.is_err());
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn redirects_are_not_followed_so_the_token_stays_with_the_agent() {
    let dir = tempfile::tempdir().unwrap();
    let client = AgentClient::new(agent_token_files(dir.path(), ["box"]), ["box"]).unwrap();
    let elsewhere_seen = Seen::default();
    let elsewhere = serve(
        Router::new()
            .route("/{*path}", any(|| async { axum::Json(agent_status()) }))
            .layer(middleware::from_fn_with_state(
                elsewhere_seen.clone(),
                record,
            )),
    )
    .await;
    let target = format!("http://{elsewhere}/status");
    let redirecting =
        serve(Router::new().route(
            "/{*path}",
            any(move || {
                let target = target.clone();
                async move {
                    (StatusCode::TEMPORARY_REDIRECT, [(header::LOCATION, target)]).into_response()
                }
            }),
        ))
        .await;
    let agent = endpoint("box", redirecting);

    assert!(client.status(&agent).await.is_err());
    assert_eq!(
        client.apply(&agent, &apply_request()).await.unwrap(),
        StatusCode::TEMPORARY_REDIRECT
    );
    assert!(client.log(&agent, "a1", 0).await.is_err());
    assert!(client.activations(&agent, 0).await.is_err());
    assert!(elsewhere_seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn errors_never_contain_the_token() {
    let dir = tempfile::tempdir().unwrap();
    let client = AgentClient::new(agent_token_files(dir.path(), ["box"]), ["box"]).unwrap();
    let refusing =
        serve(Router::new().route("/{*path}", any(|| async { StatusCode::UNAUTHORIZED }))).await;
    let agent = endpoint("box", refusing);
    for error in [
        client.status(&agent).await.unwrap_err(),
        client.log(&agent, "a1", 0).await.unwrap_err(),
        client.activations(&agent, 0).await.unwrap_err(),
    ] {
        let text = format!("{error:#} {error:?}");
        assert!(!text.contains(AGENT_TOKEN), "{text}");
    }
    let closed = endpoint("box", "127.0.0.1:1".parse().unwrap());
    let error = client.status(&closed).await.unwrap_err();
    assert!(!format!("{error:#} {error:?}").contains(AGENT_TOKEN));
}

#[test]
fn token_files_are_read_from_the_config_never_inlined() {
    let config: lodestar_server::config::ServerConfig = serde_json::from_str(
        r#"{"listen":"127.0.0.1:8080","agent_port":7461,"hosts_file":"/h.json",
        "repo_url":"u","push_url":"p","state_dir":"/s","access":{"kind":"disabled"},
        "online":{"kind":"tailscale"},
        "agent_token_files":{"box":"/run/secrets/lodestar-agent-box"}}"#,
    )
    .unwrap();
    assert_eq!(
        config.agent_token_files,
        BTreeMap::from([(
            "box".to_string(),
            PathBuf::from("/run/secrets/lodestar-agent-box")
        )])
    );
}

#[test]
fn every_token_file_is_checked_before_the_server_starts() {
    let dir = tempfile::tempdir().unwrap();
    let files = agent_token_files(dir.path(), ["box", "laptop"]);
    assert!(AgentClient::new(files.clone(), ["box", "laptop"]).is_ok());
    let broken = |content: Option<&str>, mode: u32| {
        let path = files["laptop"].clone();
        let _ = std::fs::remove_file(&path);
        if let Some(content) = content {
            std::fs::write(&path, content).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        }
        let error = AgentClient::new(files.clone(), ["box", "laptop"])
            .err()
            .map(|error| format!("{error:#}"))
            .expect("must refuse to start");
        assert!(error.contains("laptop"), "{error}");
        assert!(!error.contains(AGENT_TOKEN), "{error}");
    };
    broken(None, 0o600);
    broken(Some(AGENT_TOKEN), 0o644);
    broken(Some("short"), 0o600);
    broken(Some(&format!("{AGENT_TOKEN}\n{OTHER_TOKEN}\n")), 0o600);
}
