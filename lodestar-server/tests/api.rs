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
use lodestar_server::builds::Builder;
use lodestar_server::config::{
    AccessConfig, BuildsConfig, Link, LinkCheck, ManagedHost, OnlineSource, ServerConfig,
    SoftwareConfig, UpdatesConfig, Upstream,
};
use lodestar_server::db;
use lodestar_server::db::entities::applies;
use lodestar_server::mirror::Mirror;
use lodestar_server::state::AppState;
use lodestar_server::tailscale::StaticOnline;
use lodestar_server::updates::nix_runner::NixRunner;
use lodestar_server::updates::upstream::LatestVersion;
use sea_orm::EntityTrait;
use serde::Deserialize;
use serde_json::{Value, json};
use support::{FakeBuilder, Origin, git};

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
        node_id: None,
        dns_name: None,
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
    env_build(
        dir,
        repository_url,
        agent,
        hosts,
        access,
        links,
        Default::default(),
        None,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn env_build(
    dir: tempfile::TempDir,
    repository_url: &str,
    agent: SocketAddr,
    hosts: Vec<ManagedHost>,
    access: AccessConfig,
    links: Vec<Link>,
    updates: UpdatesConfig,
    nix: Option<Arc<dyn NixRunner>>,
    builder: Option<(Arc<dyn Builder>, bool)>,
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
        updates,
        builds: BuildsConfig {
            enable: builder.as_ref().is_some_and(|(_, enable)| *enable),
            ..Default::default()
        },
        metrics: None,
        nix_bin: "nix".into(),
    };
    let connection = db::connect(&config.state_dir).await.unwrap();
    let mirror = Arc::new(Mirror::open_or_init(&config).await.unwrap());
    let online = Arc::new(StaticOnline::new(&addresses).unwrap());
    let mut state = AppState::new(Arc::new(config), connection, mirror, online, hosts).unwrap();
    state.links = Arc::new(links);
    if let Some(nix) = nix {
        state.nix = nix;
    }
    if let Some((builder, _)) = builder {
        state.builder = builder;
    }
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

/// Nix whose `flake_update` waits until the test releases it.
struct BlockedNix {
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}

#[async_trait::async_trait]
impl NixRunner for BlockedNix {
    async fn flake_update(&self, _dir: &std::path::Path, _input: &str) -> anyhow::Result<()> {
        self.entered.notify_one();
        self.release.notified().await;
        Ok(())
    }

    async fn eval_versions(
        &self,
        _dir: &std::path::Path,
        _host: &ManagedHost,
    ) -> anyhow::Result<std::collections::BTreeMap<String, String>> {
        Ok(Default::default())
    }

    async fn build(&self, _dir: &std::path::Path, _host: &ManagedHost) -> Result<(), String> {
        Ok(())
    }
}

#[tokio::test]
async fn updates_view_and_run_endpoint() {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    std::fs::write(origin.work.join("flake.lock"), "{}\n").unwrap();
    git(&origin.work, &["add", "flake.lock"]);
    origin.commit("lock", "unstable");
    let agent = fake_agent(Shared::default()).await;
    let updates = UpdatesConfig {
        software: std::collections::BTreeMap::from([(
            "tailscale".to_string(),
            SoftwareConfig {
                input: "nixpkgs".into(),
                upstream: Upstream::Github("tailscale/tailscale".into()),
            },
        )]),
        ..Default::default()
    };
    let (entered, release) = (
        Arc::new(tokio::sync::Notify::new()),
        Arc::new(tokio::sync::Notify::new()),
    );
    let nix = Arc::new(BlockedNix {
        entered: entered.clone(),
        release: release.clone(),
    });
    let environment = env_build(
        dir,
        origin.bare.to_str().unwrap(),
        agent,
        vec![host("H1", "unstable")],
        AccessConfig::Disabled,
        Vec::new(),
        updates,
        Some(nix),
        None,
    )
    .await;
    environment.state.latest.lock().unwrap().insert(
        "tailscale".into(),
        LatestVersion {
            version: Some("1.98.10".into()),
            checked_at: Some(7),
            error: None,
            errors: 0,
        },
    );
    let http = dashboard_client();
    let url = format!("{}/api/updates", environment.base);
    let view: lodestar_proto::UpdatesView =
        http.get(&url).send().await.unwrap().json().await.unwrap();
    assert_eq!(
        view,
        lodestar_proto::UpdatesView {
            software: vec![lodestar_proto::SoftwareView {
                name: "tailscale".into(),
                input: "nixpkgs".into(),
                latest: Some("1.98.10".into()),
                checked_at: Some(7),
                error: None,
            }],
            inputs: vec![lodestar_proto::InputView {
                name: "nixpkgs".into(),
                last: None,
            }],
            running: false,
        }
    );

    let run_url = format!("{url}/run");
    let response = http.post(&run_url).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    // No waiting: the lock is taken before the 202.
    let view: lodestar_proto::UpdatesView =
        http.get(&url).send().await.unwrap().json().await.unwrap();
    assert!(view.running, "running right after the 202");
    entered.notified().await;
    let view: lodestar_proto::UpdatesView =
        http.get(&url).send().await.unwrap().json().await.unwrap();
    assert!(view.running);
    let response = http.post(&run_url).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body, json!({"error": "already running"}));

    release.notify_one();
    let mut view = view;
    for _ in 0..100 {
        view = http.get(&url).send().await.unwrap().json().await.unwrap();
        if !view.running && view.inputs[0].last.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let last = view.inputs[0].last.clone().expect("run recorded");
    assert_eq!(last.result, "no-change");
    let audit: Vec<AuditEvent> = http
        .get(format!("{}/api/audit", environment.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let requested = audit_kinds(&audit, "update-requested");
    assert_eq!(requested.len(), 1, "{audit:?}");
    assert_eq!(requested[0].actor, "dev");
    // The 409 was not audited.
    let response = http.post(&run_url).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
}

#[tokio::test]
async fn run_endpoint_is_refused_when_the_updater_is_off() {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    origin.commit("first", "unstable");
    let agent = fake_agent(Shared::default()).await;
    let environment = env_build(
        dir,
        origin.bare.to_str().unwrap(),
        agent,
        vec![host("H1", "unstable")],
        AccessConfig::Disabled,
        Vec::new(),
        UpdatesConfig::default(),
        None,
        None,
    )
    .await;
    let http = dashboard_client();
    let response = http
        .post(format!("{}/api/updates/run", environment.base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body, json!({"error": "updater off"}));
    assert!(!environment.state.updates.is_running());
}

fn darwin(name: &str, channel: &str) -> ManagedHost {
    ManagedHost {
        system: "aarch64-darwin".into(),
        os: "darwin".into(),
        node_id: None,
        dns_name: None,
        ..host(name, channel)
    }
}

/// The dashboard server with the build gate on (or off) and `builder` as its builder.
async fn gated_env(
    dir: tempfile::TempDir,
    origin: &Origin,
    agent: SocketAddr,
    hosts: Vec<ManagedHost>,
    builder: Arc<FakeBuilder>,
    enable: bool,
) -> Env {
    env_build(
        dir,
        origin.bare.to_str().unwrap(),
        agent,
        hosts,
        AccessConfig::Disabled,
        Vec::new(),
        Default::default(),
        None,
        Some((builder, enable)),
    )
    .await
}

fn names(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| name.to_string()).collect()
}

#[tokio::test]
async fn applies_wait_for_the_gate_with_every_linux_host_of_the_channel() {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    origin.commit("stable one", "stable");
    let head = origin.commit("main two", "main");
    let fake = Shared::default();
    let agent = fake_agent(fake.clone()).await;
    let hold = Arc::new(tokio::sync::Semaphore::new(0));
    let builder = Arc::new(FakeBuilder::held(hold.clone()));
    let hosts = vec![
        host("H1", "main"),
        host("H2", "stable"),
        darwin("M1", "main"),
        host("H3", "main"),
    ];
    let environment = gated_env(dir, &origin, agent, hosts, builder.clone(), true).await;
    let http = dashboard_client();

    let request = tokio::spawn(
        http.post(format!("{}/api/hosts/H1/apply", environment.base))
            .send(),
    );
    builder.entered.notified().await;
    assert_eq!(
        environment.state.building.lock().unwrap().get("main"),
        Some(&head)
    );
    assert!(fake.lock().unwrap().applied.is_empty());
    assert!(
        db::desired(&environment.state.db, "H1")
            .await
            .unwrap()
            .is_none()
    );
    hold.add_permits(1);
    let response = request.await.unwrap().unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(builder.calls(), vec![(head.clone(), names(&["H1", "H3"]))]);
    assert!(environment.state.building.lock().unwrap().is_empty());
    assert_eq!(fake.lock().unwrap().applied.len(), 1);

    // Already built: the channel apply goes straight through.
    let response = http
        .post(format!("{}/api/channels/main/apply", environment.base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(builder.calls().len(), 1);
    assert_eq!(fake.lock().unwrap().applied.len(), 4);

    // A new head is built for the channel apply too.
    let next = origin.commit("main three", "main");
    hold.add_permits(1);
    let response = http
        .post(format!("{}/api/channels/main/apply", environment.base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(builder.calls()[1], (next.clone(), names(&["H1", "H3"])));
    let desired = db::desired(&environment.state.db, "M1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(desired.commit, next);
}

#[tokio::test]
async fn a_failed_host_refuses_applies_for_the_whole_channel() {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    let head = origin.commit("main one", "main");
    let fake = Shared::default();
    let agent = fake_agent(fake.clone()).await;
    let builder = Arc::new(FakeBuilder::failing(&["H3", "H2"]));
    let hosts = vec![host("H3", "main"), host("H1", "main"), host("H2", "main")];
    let environment = gated_env(dir, &origin, agent, hosts, builder.clone(), true).await;
    let http = dashboard_client();
    let expected = format!("build of {} failed on the builder: H2, H3", &head[..7]);

    let response = http
        .post(format!("{}/api/hosts/H1/apply", environment.base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["error"], expected.as_str(), "{body}");

    let response = http
        .post(format!("{}/api/channels/main/apply", environment.base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["error"], expected.as_str(), "{body}");

    assert_eq!(builder.calls().len(), 1, "results are final");
    assert!(fake.lock().unwrap().applied.is_empty());
    for name in ["H1", "H2", "H3"] {
        assert!(
            db::desired(&environment.state.db, name)
                .await
                .unwrap()
                .is_none()
        );
    }
    let audit = db::recent_audit(&environment.state.db, 100).await.unwrap();
    assert!(audit_kinds(&audit, "apply_requested").is_empty());
    let refused = audit_kinds(&audit, "apply-refused");
    assert_eq!(refused.len(), 2, "{audit:?}");
    assert!(refused.iter().all(|event| event.actor == "dev"));
    let details = &refused[0].details;
    assert_eq!(details["commit"], head.as_str());
    assert_eq!(details["channel"], "main");
    assert_eq!(details["failed_hosts"], json!(["H2", "H3"]));
}

#[tokio::test]
async fn a_gate_error_refuses_the_apply() {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    let head = origin.commit("main one", "main");
    let fake = Shared::default();
    let agent = fake_agent(fake.clone()).await;
    let builder = Arc::new(FakeBuilder::default());
    let environment = gated_env(
        dir,
        &origin,
        agent,
        vec![host("H1", "main")],
        builder.clone(),
        true,
    )
    .await;
    let builds = environment.state.config.state_dir.join("builds");
    std::fs::create_dir_all(&builds).unwrap();
    std::fs::write(builds.join("channels.json"), "not json").unwrap();

    let response = dashboard_client()
        .post(format!("{}/api/hosts/H1/apply", environment.base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body: Value = response.json().await.unwrap();
    let message = body["error"].as_str().unwrap();
    assert!(
        message.starts_with(&format!("build of {} failed: ", &head[..7])),
        "{message}"
    );
    assert!(builder.calls().is_empty());
    assert!(fake.lock().unwrap().applied.is_empty());
    assert!(
        db::desired(&environment.state.db, "H1")
            .await
            .unwrap()
            .is_none()
    );
    let audit = db::recent_audit(&environment.state.db, 100).await.unwrap();
    let refused = audit_kinds(&audit, "apply-refused");
    assert_eq!(refused.len(), 1, "{audit:?}");
    assert_eq!(refused[0].host.as_deref(), Some("H1"));
    assert!(refused[0].details["error"].is_string(), "{:?}", refused[0]);
    assert!(environment.state.building.lock().unwrap().is_empty());
}

/// Not reaching the builder, or not publishing, refuses the apply with the reason and records no
/// result, so the next apply builds again.
#[tokio::test]
async fn an_unreachable_builder_or_failed_publish_refuses_without_a_result() {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    let head = origin.commit("main one", "main");
    let fake = Shared::default();
    let agent = fake_agent(fake.clone()).await;
    let builder = Arc::new(FakeBuilder::default());
    builder.set_unreachable(true);
    let environment = gated_env(
        dir,
        &origin,
        agent,
        vec![host("H1", "main")],
        builder.clone(),
        true,
    )
    .await;
    let http = dashboard_client();
    let apply = || async {
        let response = http
            .post(format!("{}/api/hosts/H1/apply", environment.base))
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        (
            status,
            body["error"].as_str().unwrap_or_default().to_string(),
        )
    };

    let (status, message) = apply().await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        message.starts_with(&format!(
            "build of {} failed: builder unreachable: ",
            &head[..7]
        )),
        "{message}"
    );
    assert!(environment.state.builds.read(&head).is_none());

    builder.set_unreachable(false);
    *builder.publish_error.lock().unwrap() = Some("cache-publish: disk full".into());
    let (status, message) = apply().await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        message,
        format!(
            "build of {} failed: publish failed: cache-publish: disk full",
            &head[..7]
        )
    );
    assert!(environment.state.builds.read(&head).is_none());
    assert_eq!(builder.calls().len(), 2);

    *builder.publish_error.lock().unwrap() = None;
    let (status, message) = apply().await;
    assert_eq!(status, StatusCode::ACCEPTED, "{message}");
    assert_eq!(builder.calls().len(), 3);
    assert_eq!(fake.lock().unwrap().applied.len(), 1);
    let audit = db::recent_audit(&environment.state.db, 100).await.unwrap();
    assert_eq!(audit_kinds(&audit, "apply-refused").len(), 2);
}

#[tokio::test]
async fn a_darwin_only_channel_never_waits_on_the_gate() {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    origin.commit("mac one", "mac");
    let fake = Shared::default();
    let agent = fake_agent(fake.clone()).await;
    // Never released: a gate call would hang the apply.
    let builder = Arc::new(FakeBuilder::held(Arc::new(tokio::sync::Semaphore::new(0))));
    let hosts = vec![darwin("M1", "mac"), darwin("M2", "mac"), host("H1", "main")];
    let environment = gated_env(dir, &origin, agent, hosts, builder.clone(), true).await;
    let http = dashboard_client();

    for path in ["hosts/M1/apply", "channels/mac/apply"] {
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            http.post(format!("{}/api/{path}", environment.base)).send(),
        )
        .await
        .expect("a darwin-only apply waited")
        .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED, "{path}");
    }
    assert!(builder.calls().is_empty());
    assert!(!environment.state.config.state_dir.join("builds").exists());
    assert_eq!(fake.lock().unwrap().applied.len(), 3);
}

#[tokio::test]
async fn builds_off_never_calls_the_builder() {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    origin.commit("main one", "main");
    let fake = Shared::default();
    let agent = fake_agent(fake.clone()).await;
    let builder = Arc::new(FakeBuilder::failing(&["H1"]));
    let environment = gated_env(
        dir,
        &origin,
        agent,
        vec![host("H1", "main")],
        builder.clone(),
        false,
    )
    .await;

    let response = dashboard_client()
        .post(format!("{}/api/hosts/H1/apply", environment.base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert!(builder.calls().is_empty());
    assert_eq!(fake.lock().unwrap().applied.len(), 1);
}

#[tokio::test]
async fn a_darwin_apply_on_a_mixed_channel_skips_the_gate() {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    let head = origin.commit("main one", "main");
    let fake = Shared::default();
    let agent = fake_agent(fake.clone()).await;
    // Never released: the Linux apply stays building throughout.
    let builder = Arc::new(FakeBuilder::held(Arc::new(tokio::sync::Semaphore::new(0))));
    let hosts = vec![host("H1", "main"), darwin("M1", "main")];
    let environment = gated_env(dir, &origin, agent, hosts, builder.clone(), true).await;
    let http = dashboard_client();

    let linux = tokio::spawn(
        http.post(format!("{}/api/hosts/H1/apply", environment.base))
            .send(),
    );
    builder.entered.notified().await;
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        http.post(format!("{}/api/hosts/M1/apply", environment.base))
            .send(),
    )
    .await
    .expect("the darwin apply waited on the gate")
    .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let desired = db::desired(&environment.state.db, "M1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(desired.commit, head);
    assert_eq!(fake.lock().unwrap().applied.len(), 1);
    assert!(
        db::desired(&environment.state.db, "H1")
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(builder.calls().len(), 1);
    linux.abort();
}

#[tokio::test]
async fn a_head_that_moves_during_the_build_is_built_before_it_is_desired() {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    let first = origin.commit("main one", "main");
    let fake = Shared::default();
    let agent = fake_agent(fake.clone()).await;
    let hold = Arc::new(tokio::sync::Semaphore::new(0));
    let builder = Arc::new(FakeBuilder::held(hold.clone()));
    let environment = gated_env(
        dir,
        &origin,
        agent,
        vec![host("H1", "main")],
        builder.clone(),
        true,
    )
    .await;

    let request = tokio::spawn(
        dashboard_client()
            .post(format!("{}/api/hosts/H1/apply", environment.base))
            .send(),
    );
    builder.entered.notified().await;
    let second = origin.commit("main two", "main");
    hold.add_permits(1);
    builder.entered.notified().await;
    assert!(
        db::desired(&environment.state.db, "H1")
            .await
            .unwrap()
            .is_none()
    );
    hold.add_permits(1);
    let response = request.await.unwrap().unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let built: Vec<String> = builder
        .calls()
        .into_iter()
        .map(|(commit, _)| commit)
        .collect();
    assert_eq!(built, vec![first, second.clone()]);
    let desired = db::desired(&environment.state.db, "H1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(desired.commit, second);
    assert_eq!(fake.lock().unwrap().applied.len(), 1);
}

#[tokio::test]
async fn a_channel_that_keeps_moving_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    origin.commit("main zero", "main");
    let fake = Shared::default();
    let agent = fake_agent(fake.clone()).await;
    let hold = Arc::new(tokio::sync::Semaphore::new(0));
    let builder = Arc::new(FakeBuilder::held(hold.clone()));
    let environment = gated_env(
        dir,
        &origin,
        agent,
        vec![host("H1", "main")],
        builder.clone(),
        true,
    )
    .await;

    let request = tokio::spawn(
        dashboard_client()
            .post(format!("{}/api/hosts/H1/apply", environment.base))
            .send(),
    );
    // Every build round sees the head move again.
    let mut last = String::new();
    for round in 0..3 {
        builder.entered.notified().await;
        last = origin.commit(&format!("main move {round}"), "main");
        hold.add_permits(1);
    }
    let response = request.await.unwrap().unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body: Value = response.json().await.unwrap();
    assert_eq!(
        body["error"], "channel main kept moving while its head was built; try again",
        "{body}"
    );
    assert_eq!(builder.calls().len(), 3);
    assert!(
        db::desired(&environment.state.db, "H1")
            .await
            .unwrap()
            .is_none()
    );
    assert!(fake.lock().unwrap().applied.is_empty());
    let audit = db::recent_audit(&environment.state.db, 100).await.unwrap();
    let refused = audit_kinds(&audit, "apply-refused");
    assert_eq!(refused.len(), 1, "{audit:?}");
    assert_eq!(refused[0].details["reason"], "channel kept moving");
    assert_eq!(refused[0].details["commit"], last.as_str());
}

async fn snapshot_channels(environment: &Env) -> Vec<lodestar_proto::ChannelView> {
    let snapshot: lodestar_proto::Snapshot = dashboard_client()
        .get(format!("{}/api/snapshot", environment.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    snapshot.channels
}

#[tokio::test]
async fn the_channel_shows_building_then_ok_in_snapshot_and_builds() {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    let head = origin.commit("main one", "main");
    let agent = fake_agent(Shared::default()).await;
    let hold = Arc::new(tokio::sync::Semaphore::new(0));
    let builder = Arc::new(FakeBuilder::held(hold.clone()));
    let environment = gated_env(
        dir,
        &origin,
        agent,
        vec![host("H1", "main")],
        builder.clone(),
        true,
    )
    .await;
    let http = dashboard_client();
    assert_eq!(snapshot_channels(&environment).await[0].build, None);

    let request = tokio::spawn(
        http.post(format!("{}/api/hosts/H1/apply", environment.base))
            .send(),
    );
    builder.entered.notified().await;
    let build = snapshot_channels(&environment).await[0].build.clone();
    assert_eq!(
        build,
        Some(lodestar_proto::ChannelBuildView {
            commit: head.clone(),
            state: "building".into(),
            failed_hosts: Vec::new(),
            finished_at: None,
        })
    );
    hold.add_permits(1);
    request.await.unwrap().unwrap();

    let build = snapshot_channels(&environment).await[0]
        .build
        .clone()
        .unwrap();
    assert_eq!(
        (build.commit.as_str(), build.state.as_str()),
        (head.as_str(), "ok")
    );
    assert!(build.failed_hosts.is_empty() && build.finished_at.is_some());
    let listed: Vec<lodestar_proto::ChannelView> = http
        .get(format!("{}/api/builds", environment.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(listed[0].name, "main");
    assert_eq!(listed[0].build, Some(build));
}

#[tokio::test]
async fn a_failed_channel_build_is_visible_and_retry_rebuilds_it() {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    let head = origin.commit("main one", "main");
    let agent = fake_agent(Shared::default()).await;
    let builder = Arc::new(FakeBuilder::failing(&["H2"]));
    let hosts = vec![host("H1", "main"), host("H2", "main")];
    let environment = gated_env(dir, &origin, agent, hosts, builder.clone(), true).await;
    let http = dashboard_client();
    let retry_url = format!("{}/api/builds/{head}/retry", environment.base);

    // No result yet: 404.
    let response = http.post(&retry_url).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = http
        .post(format!("{}/api/channels/main/apply", environment.base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let build = snapshot_channels(&environment).await[0]
        .build
        .clone()
        .unwrap();
    assert_eq!(build.state, "failed");
    assert_eq!(build.failed_hosts, names(&["H2"]));

    // The metrics carry the failed build.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let metrics_addr = listener.local_addr().unwrap();
    let metrics_app = lodestar_server::api::metrics_app(environment.state.clone());
    tokio::spawn(async move { axum::serve(listener, metrics_app).await.unwrap() });
    let metrics = reqwest::get(format!("http://{metrics_addr}/metrics"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        metrics.contains("lodestar_build_last_result{channel=\"main\",result=\"failed\"} 1\n"),
        "{metrics}"
    );
    assert!(
        metrics.contains("lodestar_build_last_timestamp_seconds{channel=\"main\"} "),
        "{metrics}"
    );
    assert!(
        metrics.contains("lodestar_build_duration_seconds{channel=\"main\"} "),
        "{metrics}"
    );

    // The recent builds list every host of the commit: H1's store path, H2's log.
    let recent: lodestar_proto::BuildsView = http
        .get(format!("{}/api/builds/recent", environment.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(recent.enabled);
    assert_eq!(recent.builds.len(), 1, "{recent:?}");
    let listed = &recent.builds[0];
    assert_eq!(listed.commit, head);
    assert_eq!(listed.subject.as_deref(), Some("main one"));
    assert_eq!(listed.head_of, names(&["main"]));
    assert!(listed.building.is_empty() && listed.cached_for.is_empty());
    assert!(listed.finished_at.is_some());
    let hosts: Vec<(&str, bool, bool, Option<&str>)> = listed
        .hosts
        .iter()
        .map(|h| (h.name.as_str(), h.ok, h.path.is_some(), h.log.as_deref()))
        .collect();
    assert_eq!(
        hosts,
        vec![
            ("H1", true, true, None),
            ("H2", false, false, Some("error: builder said no")),
        ]
    );

    // A malformed commit is a 400.
    let response = http
        .post(format!("{}/api/builds/nope/retry", environment.base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // The builder works again; retry drops the failure and the channel is rebuilt.
    builder.fail.lock().unwrap().clear();
    let response = http.post(&retry_url).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body, json!({ "retried": ["H2"] }));
    let audit = db::recent_audit(&environment.state.db, 100).await.unwrap();
    let retried = audit_kinds(&audit, "build-retry");
    assert_eq!(retried.len(), 1, "{audit:?}");
    assert_eq!(retried[0].actor, "dev");
    assert_eq!(
        retried[0].details,
        json!({ "commit": head, "hosts": ["H2"] })
    );

    // The retry kicked a prebuild of the head: no apply needed.
    for _ in 0..100 {
        let build = snapshot_channels(&environment).await[0].build.clone();
        if build.as_ref().is_some_and(|build| build.state == "ok") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let build = snapshot_channels(&environment).await[0]
        .build
        .clone()
        .unwrap();
    assert_eq!(build.state, "ok", "{build:?}");
    // H1 is realised again with it: the channel advances and publishes H1's path too.
    assert_eq!(builder.calls().len(), 2);
    assert_eq!(builder.calls()[1].1, names(&["H1", "H2"]));
}
