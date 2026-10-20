//! The dashboard HTTP API, WebSocket and embedded frontend, served on 127.0.0.1:0
//! against a fake agent and a local bare repo.

mod support;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::StreamExt;
use lodestar_proto::{
    ApplyRequest, AuditEvent, LogChunk, Me, Role, ServiceCheck, ServiceState, WsMessage,
};
use lodestar_server::config::{
    AccessConfig, Link, LinkCheck, ManagedHost, OnlineSource, ServerConfig,
};
use lodestar_server::db;
use lodestar_server::db::entities::applies;
use lodestar_server::mirror::Mirror;
use lodestar_server::state::AppState;
use lodestar_server::tailscale::StaticOnline;
use sea_orm::EntityTrait;
use serde::Deserialize;
use serde_json::{Value, json};
use support::{Origin, git};

/// A client that sends the header the dashboard sends (`X-Lodestar: 1`, CSRF guard).
fn dashboard_client() -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("x-lodestar", reqwest::header::HeaderValue::from_static("1"));
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap()
}

#[derive(Default)]
struct Fake {
    applied: Vec<ApplyRequest>,
}

type Shared = Arc<Mutex<Fake>>;

#[derive(Deserialize)]
struct LogQuery {
    apply_id: String,
    offset: u64,
}

async fn fake_agent(fake: Shared) -> SocketAddr {
    async fn apply(State(fake): State<Shared>, Json(request): Json<ApplyRequest>) -> StatusCode {
        fake.lock().unwrap().applied.push(request);
        StatusCode::ACCEPTED
    }
    async fn log(Query(query): Query<LogQuery>) -> Json<LogChunk> {
        Json(LogChunk {
            apply_id: query.apply_id,
            offset: query.offset,
            data: "live log\n".into(),
            next: query.offset + 9,
            eof: false,
        })
    }
    let app = Router::new()
        .route("/apply", post(apply))
        .route("/apply/log", get(log))
        .with_state(fake);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

fn host(name: &str, channel: &str) -> ManagedHost {
    ManagedHost {
        name: name.into(),
        system: "x86_64-linux".into(),
        user: "u".into(),
        channel: channel.into(),
        os: "nixos".into(),
    }
}

struct Env {
    state: AppState,
    base: String,
    _dir: tempfile::TempDir,
}

/// Starts the dashboard server without access control. Every host is online at `agent`.
async fn env(
    dir: tempfile::TempDir,
    repository_url: &str,
    agent: SocketAddr,
    hosts: Vec<ManagedHost>,
) -> Env {
    env_with_access(dir, repository_url, agent, hosts, AccessConfig::Disabled).await
}

async fn env_with_access(
    dir: tempfile::TempDir,
    repository_url: &str,
    agent: SocketAddr,
    hosts: Vec<ManagedHost>,
    access: AccessConfig,
) -> Env {
    env_with_links(dir, repository_url, agent, hosts, access, Vec::new()).await
}

async fn env_with_links(
    dir: tempfile::TempDir,
    repository_url: &str,
    agent: SocketAddr,
    hosts: Vec<ManagedHost>,
    access: AccessConfig,
    links: Vec<Link>,
) -> Env {
    let addresses: HashMap<String, String> = hosts
        .iter()
        .map(|managed_host| (managed_host.name.to_lowercase(), agent.to_string()))
        .collect();
    let config = ServerConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        agent_port: 7461,
        hosts_file: dir.path().join("hosts.json"),
        links_file: None,
        repo_url: repository_url.into(),
        push_url: repository_url.into(),
        deploy_key_file: None,
        state_dir: dir.path().join("state"),
        access,
        online: OnlineSource::Static {
            addresses: addresses.clone(),
        },
        mirror_fetch_secs: 300,
        poll_secs: 60,
        tailscale_bin: PathBuf::from("tailscale"),
        promote: Some(lodestar_proto::PromoteChannels::default()),
        proxy: None,
    };
    let connection = db::connect(&config.state_dir).await.unwrap();
    let mirror = Arc::new(Mirror::open_or_init(&config).await.unwrap());
    let online = Arc::new(StaticOnline::new(&addresses).unwrap());
    let mut state = AppState::new(Arc::new(config), connection, mirror, online, hosts).unwrap();
    state.links = Arc::new(links);
    let app = lodestar_server::api::app(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap()
    });
    Env {
        state,
        base: format!("http://{addr}"),
        _dir: dir,
    }
}

fn audit_kinds(events: &[AuditEvent], kind: &str) -> Vec<AuditEvent> {
    events
        .iter()
        .filter(|event| event.kind == kind)
        .cloned()
        .collect()
}

#[tokio::test]
async fn apply_endpoint_creates_desired_and_hits_agent() {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    let head = origin.commit("one", "main");
    let fake = Shared::default();
    let agent = fake_agent(fake.clone()).await;
    let environment = env(
        dir,
        origin.bare.to_str().unwrap(),
        agent,
        vec![host("H1", "main")],
    )
    .await;
    let mut receiver = environment.state.tx.subscribe();
    let http = dashboard_client();

    let response = http
        .post(format!("{}/api/hosts/H1/apply", environment.base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["agent_status"], "202", "{body}");
    let id = body["apply_id"].as_str().unwrap().to_string();
    assert_eq!(id.len(), 36, "{id}");

    let desired = db::desired(&environment.state.db, "H1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(desired.apply_id, id);
    assert_eq!(desired.commit, head);
    assert_eq!(desired.requested_by, "dev");
    assert_eq!(desired.state, "idle");
    assert_eq!(
        fake.lock().unwrap().applied,
        vec![ApplyRequest {
            apply_id: id.clone(),
            commit: head.clone()
        }]
    );
    let audit = db::recent_audit(&environment.state.db, 100).await.unwrap();
    let requested = audit_kinds(&audit, "apply_requested");
    assert_eq!(requested.len(), 1, "{audit:?}");
    assert_eq!(requested[0].actor, "dev");
    assert_eq!(requested[0].host.as_deref(), Some("H1"));
    match receiver.recv().await.unwrap() {
        WsMessage::HostUpdated(host_view) => {
            assert_eq!(host_view.desired_apply_id.as_deref(), Some(&*id))
        }
        other => panic!("unexpected {other:?}"),
    }

    // Unknown host: 404.
    let response = http
        .post(format!("{}/api/hosts/nope/apply", environment.base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // No stored log yet: the live log comes from the agent.
    let log = http
        .get(format!(
            "{}/api/hosts/H1/applies/{id}/log",
            environment.base
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(log.status(), StatusCode::OK);
    assert_eq!(log.text().await.unwrap(), "live log\n");
    db::set_apply_log(&environment.state.db, &id, "stored log\n")
        .await
        .unwrap();
    let log = http
        .get(format!(
            "{}/api/hosts/H1/applies/{id}/log",
            environment.base
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(log.text().await.unwrap(), "stored log\n");

    let audit: Vec<AuditEvent> = http
        .get(format!("{}/api/audit?limit=100", environment.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(audit_kinds(&audit, "apply_requested").len(), 1);
}

#[tokio::test]
async fn state_changing_requests_need_the_dashboard_header() {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    origin.commit("one", "main");
    let fake = Shared::default();
    let agent = fake_agent(fake.clone()).await;
    let environment = env(
        dir,
        origin.bare.to_str().unwrap(),
        agent,
        vec![host("H1", "main")],
    )
    .await;
    let bare = reqwest::Client::new();
    let url = format!("{}/api/hosts/H1/apply", environment.base);

    // No header (a cross-site form or no-cors fetch): refused, nothing applied.
    let response = bare.post(&url).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body: Value = response.json().await.unwrap();
    assert!(body["error"].is_string(), "{body}");
    let response = bare
        .post(format!("{}/api/channels/main/apply", environment.base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = bare
        .post(format!("{}/api/promote", environment.base))
        .json(&json!({"commit": "0".repeat(40)}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // The header, but the browser says the request is cross-site: refused.
    for site in ["cross-site", "same-site"] {
        let response = bare
            .post(&url)
            .header("x-lodestar", "1")
            .header("sec-fetch-site", site)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{site}");
    }
    assert!(fake.lock().unwrap().applied.is_empty());
    assert!(
        applies::Entity::find()
            .all(&environment.state.db)
            .await
            .unwrap()
            .is_empty()
    );

    // GETs need no header.
    let response = bare
        .get(format!("{}/api/snapshot", environment.base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // The header from the dashboard's own origin (or without Sec-Fetch-Site): accepted.
    let response = bare
        .post(&url)
        .header("x-lodestar", "1")
        .header("sec-fetch-site", "same-origin")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let response = bare
        .post(&url)
        .header("x-lodestar", "1")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(fake.lock().unwrap().applied.len(), 2);
}

#[tokio::test]
async fn apply_endpoint_502_when_mirror_fetch_fails() {
    let dir = tempfile::tempdir().unwrap();
    let bogus = dir.path().join("does-not-exist.git");
    let fake = Shared::default();
    let agent = fake_agent(fake.clone()).await;
    let environment = env(
        dir,
        bogus.to_str().unwrap(),
        agent,
        vec![host("H1", "main")],
    )
    .await;

    let response = dashboard_client()
        .post(format!("{}/api/hosts/H1/apply", environment.base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let body: Value = response.json().await.unwrap();
    assert!(body["error"].is_string(), "{body}");
    assert!(
        applies::Entity::find()
            .all(&environment.state.db)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        db::desired(&environment.state.db, "H1")
            .await
            .unwrap()
            .is_none()
    );
    assert!(fake.lock().unwrap().applied.is_empty());
}

#[tokio::test]
async fn channel_apply_targets_only_that_channel() {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    origin.commit("stable one", "stable");
    let head = origin.commit("main two", "main");
    let fake = Shared::default();
    let agent = fake_agent(fake.clone()).await;
    let hosts = vec![host("H1", "main"), host("H2", "stable"), host("H3", "main")];
    let environment = env(dir, origin.bare.to_str().unwrap(), agent, hosts).await;
    let http = dashboard_client();

    let response = http
        .post(format!("{}/api/channels/main/apply", environment.base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body: Value = response.json().await.unwrap();
    let ids = body["apply_ids"].as_object().unwrap();
    let mut names: Vec<&str> = ids.keys().map(String::as_str).collect();
    names.sort();
    assert_eq!(names, ["H1", "H3"]);
    for name in ["H1", "H3"] {
        let desired = db::desired(&environment.state.db, name)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(desired.apply_id, ids[name].as_str().unwrap());
        assert_eq!(desired.commit, head);
    }
    assert!(
        db::desired(&environment.state.db, "H2")
            .await
            .unwrap()
            .is_none()
    );
    let applied = fake.lock().unwrap().applied.clone();
    assert_eq!(applied.len(), 2);
    assert!(applied.iter().all(|request| request.commit == head));
    let audit = db::recent_audit(&environment.state.db, 100).await.unwrap();
    assert_eq!(audit_kinds(&audit, "apply_requested").len(), 2);

    let response = http
        .post(format!("{}/api/channels/nope/apply", environment.base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn promote_moves_stable_and_audits() {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    let first_commit = origin.commit("one", "stable");
    origin.push("unstable");
    let second_commit = origin.commit("two", "unstable");
    git(
        &origin.work,
        &["checkout", "-q", "-b", "side", &first_commit],
    );
    let side = origin.commit("side", "side");
    let fake = Shared::default();
    let agent = fake_agent(fake).await;
    let environment = env(
        dir,
        origin.bare.to_str().unwrap(),
        agent,
        vec![host("H1", "stable")],
    )
    .await;
    let mut receiver = environment.state.tx.subscribe();
    let http = dashboard_client();
    let promote = |body: Value| {
        let (http, url) = (http.clone(), format!("{}/api/promote", environment.base));
        async move { http.post(url).json(&body).send().await.unwrap() }
    };

    assert_eq!(
        promote(json!({"commit": "--delete"})).await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        promote(json!({"commit": side})).await.status(),
        StatusCode::CONFLICT
    );
    assert_eq!(origin.bare_ref("stable"), first_commit);

    let response = promote(json!({"commit": second_commit})).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(origin.bare_ref("stable"), second_commit);
    let audit = db::recent_audit(&environment.state.db, 100).await.unwrap();
    let promotes = audit_kinds(&audit, "promote");
    assert_eq!(promotes.len(), 1, "{audit:?}");
    assert_eq!(promotes[0].actor, "dev");
    assert_eq!(promotes[0].details["commit"], second_commit);
    match receiver.recv().await.unwrap() {
        WsMessage::ChannelsUpdated(channels) => {
            let stable = channels
                .iter()
                .find(|channel| channel.name == "stable")
                .unwrap();
            assert_eq!(stable.head.as_deref(), Some(&*second_commit));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn ws_sends_snapshot_first() {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    origin.commit("one", "main");
    let agent = fake_agent(Shared::default()).await;
    let environment = env(
        dir,
        origin.bare.to_str().unwrap(),
        agent,
        vec![host("H1", "main")],
    )
    .await;

    let url = format!("{}/api/ws", environment.base.replacen("http", "ws", 1));
    let (mut socket, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    let first = socket.next().await.unwrap().unwrap();
    let message: WsMessage = serde_json::from_str(first.to_text().unwrap()).unwrap();
    match message {
        WsMessage::Snapshot(snapshot) => {
            assert_eq!(snapshot.viewer, "dev");
            assert_eq!(snapshot.hosts.len(), 1);
            assert_eq!(snapshot.hosts[0].name, "H1");
        }
        other => panic!("unexpected {other:?}"),
    }

    // Broadcasts are forwarded after the snapshot.
    let channels = lodestar_server::state::channels(&environment.state).await;
    environment
        .state
        .broadcast(WsMessage::ChannelsUpdated(channels.clone()));
    let next = socket.next().await.unwrap().unwrap();
    let message: WsMessage = serde_json::from_str(next.to_text().unwrap()).unwrap();
    assert_eq!(message, WsMessage::ChannelsUpdated(channels));
    socket.close(None).await.unwrap();
}

#[tokio::test]
async fn spa_fallback_serves_index() {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    origin.commit("one", "main");
    let agent = fake_agent(Shared::default()).await;
    let environment = env(
        dir,
        origin.bare.to_str().unwrap(),
        agent,
        vec![host("H1", "main")],
    )
    .await;
    let http = dashboard_client();

    for path in ["/", "/index.html", "/hosts/H1"] {
        let response = http
            .get(format!("{}{path}", environment.base))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let content_type = response.headers()["content-type"]
            .to_str()
            .unwrap()
            .to_string();
        assert!(
            content_type.starts_with("text/html"),
            "{path}: {content_type}"
        );
        assert!(
            response.text().await.unwrap().contains(r#"id="app""#),
            "{path}"
        );
    }
    // Unknown API routes are not swallowed by the SPA fallback.
    let response = http
        .get(format!("{}/api/nope", environment.base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let snapshot: lodestar_proto::Snapshot = http
        .get(format!("{}/api/snapshot", environment.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(snapshot.viewer, "dev");
}

#[tokio::test]
async fn access_denied_on_api_ws_and_static() {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    origin.commit("one", "main");
    let fake = Shared::default();
    let agent = fake_agent(fake.clone()).await;
    // StaticOnline has no whois identities, so Tailscale access control denies everyone.
    let access = AccessConfig::Tailscale {
        allowed_users: vec!["alice@example.com".into()],
        allowed_tags: vec!["tag:lodestar".into()],
    };
    let environment = env_with_access(
        dir,
        origin.bare.to_str().unwrap(),
        agent,
        vec![host("H1", "main")],
        access,
    )
    .await;
    let http = dashboard_client();

    for path in [
        "/",
        "/index.html",
        "/hosts/H1",
        "/api/snapshot",
        "/api/audit",
    ] {
        let response = http
            .get(format!("{}{path}", environment.base))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
    }
    let response = http
        .post(format!("{}/api/hosts/H1/apply", environment.base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(fake.lock().unwrap().applied.is_empty());

    let url = format!("{}/api/ws", environment.base.replacen("http", "ws", 1));
    match tokio_tungstenite::connect_async(url).await {
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
            assert_eq!(response.status(), StatusCode::FORBIDDEN)
        }
        other => panic!("expected 403 on upgrade, got {other:?}"),
    }
}

#[tokio::test]
async fn concurrent_applies_all_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    origin.commit("one", "main");
    let fake = Shared::default();
    let agent = fake_agent(fake.clone()).await;
    let names: Vec<String> = (1..=8).map(|index| format!("H{index}")).collect();
    let hosts = names.iter().map(|name| host(name, "main")).collect();
    let environment = env(dir, origin.bare.to_str().unwrap(), agent, hosts).await;
    // A push right before the presses, so every fetch has refs to write.
    let head = origin.commit("two", "main");
    let http = dashboard_client();

    let presses = names.iter().map(|name| {
        let (http, url) = (
            http.clone(),
            format!("{}/api/hosts/{name}/apply", environment.base),
        );
        async move { http.post(url).send().await.unwrap().status() }
    });
    // The background fetch runs at the same time.
    let background = async {
        for _ in 0..4 {
            environment.state.mirror.fetch().await.unwrap();
        }
    };
    let (statuses, ()) = tokio::join!(futures::future::join_all(presses), background);
    assert!(
        statuses
            .iter()
            .all(|status| *status == StatusCode::ACCEPTED),
        "{statuses:?}"
    );
    for name in &names {
        let desired = db::desired(&environment.state.db, name)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(desired.commit, head, "{name}");
    }
    assert_eq!(fake.lock().unwrap().applied.len(), names.len());
}

#[tokio::test]
async fn me_on_tailnet_entrance_is_admin() {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    origin.commit("one", "main");
    let agent = fake_agent(Shared::default()).await;
    let environment = env(dir, origin.bare.to_str().unwrap(), agent, vec![]).await;

    let response = dashboard_client()
        .get(format!("{}/api/me", environment.base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let me: Me = response.json().await.unwrap();
    assert_eq!(me.viewer.as_deref(), Some("dev"));
    assert_eq!(me.role, Role::Admin);
    assert!(me.can_act);
    assert!(!me.session);
    assert_eq!(me.reason, None);
}

#[tokio::test]
async fn public_services_has_only_public_fields() {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    origin.commit("one", "main");
    let agent = fake_agent(Shared::default()).await;
    let link = Link {
        id: "grafana".into(),
        title: "Grafana".into(),
        url: "https://g.example.com".into(),
        group: "Monitoring".into(),
        description: Some("Dashboards".into()),
        icon: Some("chart".into()),
        host: "media-vm".into(),
        check: LinkCheck {
            enable: true,
            url: None,
            expect_status: vec![200],
            insecure_tls: false,
        },
    };
    let environment = env_with_links(
        dir,
        origin.bare.to_str().unwrap(),
        agent,
        vec![],
        AccessConfig::Disabled,
        vec![link],
    )
    .await;
    environment.state.checks.lock().unwrap().insert(
        "grafana".into(),
        ServiceCheck {
            state: ServiceState::Down,
            reason: None,
            latency_ms: Some(12),
            checked_at: Some(1_700_000_100),
            since: Some(1_700_000_000),
        },
    );

    let response = dashboard_client()
        .get(format!("{}/api/public/services", environment.base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let services: Vec<Value> = response.json().await.unwrap();
    assert_eq!(services.len(), 1);
    let service = services[0].as_object().unwrap();
    let mut keys: Vec<&str> = service.keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(
        keys,
        ["description", "group", "icon", "since", "state", "title"]
    );
    assert_eq!(service["state"], "down");
    assert_eq!(service["since"], 1_700_000_000);
}
