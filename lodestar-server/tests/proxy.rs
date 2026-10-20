//! The router entrance: trusted-peer gate, session viewers, the tailnet mark, the WebSocket
//! Origin check and session recheck, served on 127.0.0.1:0 against a fake agent.

mod support;

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use futures::StreamExt;
use lodestar_proto::{ApplyRequest, Me, Role, Snapshot, WsMessage};
use lodestar_server::auth::{AuthState, LoginSession, SessionId};
use lodestar_server::config::{
    AccessConfig, ManagedHost, OidcConfig, OnlineSource, ProxyConfig, ServerConfig,
};
use lodestar_server::db;
use lodestar_server::db::entities::sessions;
use lodestar_server::mirror::Mirror;
use lodestar_server::state::AppState;
use lodestar_server::tailscale::StaticOnline;
use sea_orm::EntityTrait;
use serde_json::Value;
use support::Origin;
use tokio_tungstenite::tungstenite;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

const PUBLIC_URL: &str = "https://lodestar.example.com";
const MARK: &str = "x-homelab-via";

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

/// An agent that accepts every apply.
async fn fake_agent() -> SocketAddr {
    async fn apply(Json(_request): Json<ApplyRequest>) -> StatusCode {
        StatusCode::ACCEPTED
    }
    let app = Router::new().route("/apply", post(apply));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

fn proxy_config(listen: SocketAddr, trusted: &[&str], dir: &Path) -> ProxyConfig {
    let secret_file = dir.join("client-secret");
    std::fs::write(&secret_file, "s3cret\n").unwrap();
    ProxyConfig {
        listen,
        trusted_proxies: trusted
            .iter()
            .map(|ip| ip.parse::<IpAddr>().unwrap())
            .collect(),
        public_url: PUBLIC_URL.into(),
        oidc: OidcConfig {
            issuer: "https://id.example.com/oauth2/openid/lodestar".into(),
            client_id: "lodestar".into(),
            client_secret_file: secret_file,
            reader_group: "lodestar-users@example.com".into(),
            admin_group: "admins@example.com".into(),
        },
    }
}

fn server_config(dir: &Path, origin: &Origin, agent: SocketAddr) -> ServerConfig {
    ServerConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        agent_port: 7461,
        hosts_file: dir.join("hosts.json"),
        links_file: None,
        repo_url: origin.bare.to_str().unwrap().into(),
        push_url: origin.bare.to_str().unwrap().into(),
        deploy_key_file: None,
        state_dir: dir.join("state"),
        access: AccessConfig::Disabled,
        online: OnlineSource::Static {
            addresses: HashMap::from([("h".to_string(), agent.to_string())]),
        },
        mirror_fetch_secs: 300,
        poll_secs: 60,
        tailscale_bin: PathBuf::from("tailscale"),
        promote: Some(lodestar_proto::PromoteChannels::default()),
        proxy: None,
    }
}

fn host() -> ManagedHost {
    ManagedHost {
        name: "h".into(),
        system: "x86_64-linux".into(),
        user: "u".into(),
        channel: "main".into(),
        os: "nixos".into(),
    }
}

struct Env {
    state: AppState,
    base: String,
    _dir: tempfile::TempDir,
}

/// The router entrance alone (`proxy_app`) with `trusted` as its trusted proxies and host
/// `h` on channel `main`.
async fn env_with(trusted: &[&str], session_recheck: Duration) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    origin.commit("one", "main");
    let agent = fake_agent().await;
    let mut config = server_config(dir.path(), &origin, agent);
    let proxy = proxy_config("127.0.0.1:0".parse().unwrap(), trusted, dir.path());
    config.proxy = Some(proxy.clone());
    let addresses = HashMap::from([("h".to_string(), agent.to_string())]);
    let connection = db::connect(&config.state_dir).await.unwrap();
    let mirror = Arc::new(Mirror::open_or_init(&config).await.unwrap());
    let online = Arc::new(StaticOnline::new(&addresses).unwrap());
    let mut state =
        AppState::new(Arc::new(config), connection, mirror, online, vec![host()]).unwrap();
    let mut auth = AuthState::new(proxy).unwrap();
    auth.session_recheck = session_recheck;
    state.auth = Some(Arc::new(auth));
    let app = lodestar_server::api::proxy_app(state.clone());
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

async fn env() -> Env {
    env_with(&["127.0.0.1"], Duration::from_secs(60)).await
}

/// Inserts a live session for `user` and returns its cookie header value.
async fn login(state: &AppState, user: &str, role: Role, expires_at: i64) -> (String, String) {
    let id = SessionId::new_random();
    let session = LoginSession {
        id_hash: id.hash(),
        user: user.into(),
        role,
        expires_at,
    };
    db::insert_session(&state.db, &session, now())
        .await
        .unwrap();
    (
        format!("__Host-lodestar_session={}", id.as_str()),
        id.hash(),
    )
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

async fn error_of(response: reqwest::Response) -> String {
    let body: Value = response.json().await.unwrap();
    body["error"].as_str().unwrap_or_default().to_string()
}

async fn me(base: &str, cookie: Option<&str>) -> Me {
    let mut request = client().get(format!("{base}/api/me"));
    if let Some(cookie) = cookie {
        request = request.header("cookie", cookie);
    }
    request.send().await.unwrap().json().await.unwrap()
}

/// Opens `/api/ws` with optional cookie and Origin; the HTTP status of a refused upgrade on
/// `Err`.
async fn open_ws(
    base: &str,
    cookie: Option<&str>,
    origin: Option<&str>,
) -> Result<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    StatusCode,
> {
    let url = format!("{}/api/ws", base.replacen("http", "ws", 1));
    let mut request = url.into_client_request().unwrap();
    if let Some(cookie) = cookie {
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
    }
    if let Some(origin) = origin {
        request
            .headers_mut()
            .insert("origin", origin.parse().unwrap());
    }
    match tokio_tungstenite::connect_async(request).await {
        Ok((socket, response)) => {
            assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
            Ok(socket)
        }
        Err(tungstenite::Error::Http(response)) => Err(response.status()),
        Err(other) => panic!("websocket: {other:?}"),
    }
}

#[tokio::test]
async fn untrusted_peer_gets_403_everywhere() {
    let environment = env_with(&["192.0.2.1"], Duration::from_secs(60)).await;
    for path in ["/", "/api/me", "/api/public/services", "/auth/login"] {
        let response = client()
            .get(format!("{}{path}", environment.base))
            .header(MARK, "tailnet")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
        assert_eq!(response.text().await.unwrap(), "forbidden", "{path}");
    }
}

#[tokio::test]
async fn anonymous_gets_ui_me_and_public_feed_only() {
    let environment = env().await;
    let base = &environment.base;

    let index = client().get(format!("{base}/")).send().await.unwrap();
    assert_eq!(index.status(), StatusCode::OK);
    assert!(index.text().await.unwrap().contains(r#"id="app""#));

    let me = me(base, None).await;
    assert_eq!(me.role, Role::Anonymous);
    assert_eq!(me.viewer, None);

    let feed = client()
        .get(format!("{base}/api/public/services"))
        .send()
        .await
        .unwrap();
    assert_eq!(feed.status(), StatusCode::OK);

    for path in ["/api/snapshot", "/api/audit"] {
        let response = client().get(format!("{base}{path}")).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
        assert_eq!(error_of(response).await, "log in first", "{path}");
    }
    assert_eq!(
        open_ws(base, None, Some(PUBLIC_URL)).await.err(),
        Some(StatusCode::UNAUTHORIZED)
    );
}

#[tokio::test]
async fn reader_reads_but_cannot_act() {
    let environment = env().await;
    let base = &environment.base;
    let (cookie, _) = login(&environment.state, "alice", Role::Reader, now() + 3600).await;

    let response = client()
        .get(format!("{base}/api/snapshot"))
        .header("cookie", &cookie)
        .header(MARK, "tailnet")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let snapshot: Snapshot = response.json().await.unwrap();
    assert_eq!(snapshot.viewer, "alice");

    let response = client()
        .post(format!("{base}/api/hosts/h/apply"))
        .header("cookie", &cookie)
        .header(MARK, "tailnet")
        .header("x-lodestar", "1")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(error_of(response).await, "only admins can do this");
}

#[tokio::test]
async fn admin_acts_only_with_exact_mark() {
    let environment = env().await;
    let base = &environment.base;
    let (cookie, _) = login(&environment.state, "bob", Role::Admin, now() + 3600).await;
    let apply = |mark: Option<&'static str>| {
        let mut request = client()
            .post(format!("{base}/api/hosts/h/apply"))
            .header("cookie", &cookie)
            .header("x-lodestar", "1");
        if let Some(mark) = mark {
            request = request.header(MARK, mark);
        }
        async move { request.send().await.unwrap() }
    };

    for mark in [None, Some("Tailnet"), Some("yes")] {
        let response = apply(mark).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{mark:?}");
        assert_eq!(error_of(response).await, "only over Tailscale", "{mark:?}");
    }
    assert!(
        db::recent_audit(&environment.state.db, 100)
            .await
            .unwrap()
            .is_empty()
    );

    let response = apply(Some("tailnet")).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let audit = db::recent_audit(&environment.state.db, 100).await.unwrap();
    let requested: Vec<_> = audit
        .iter()
        .filter(|event| event.kind == "apply_requested")
        .collect();
    assert_eq!(requested.len(), 1, "{audit:?}");
    assert_eq!(requested[0].actor, "bob");
}

#[tokio::test]
async fn expired_session_is_anonymous() {
    let environment = env().await;
    let (cookie, hash) = login(&environment.state, "bob", Role::Admin, now() - 1).await;

    let me = me(&environment.base, Some(&cookie)).await;
    assert_eq!(me.role, Role::Anonymous);
    assert!(!me.session);
    assert!(
        sessions::Entity::find_by_id(hash)
            .one(&environment.state.db)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn websocket_origin_must_be_public_url() {
    let environment = env().await;
    let base = &environment.base;
    let (cookie, _) = login(&environment.state, "bob", Role::Admin, now() + 3600).await;

    for origin in [Some("https://evil.example.com"), None] {
        assert_eq!(
            open_ws(base, Some(&cookie), origin).await.err(),
            Some(StatusCode::FORBIDDEN),
            "{origin:?}"
        );
    }
    let mut socket = open_ws(base, Some(&cookie), Some(PUBLIC_URL))
        .await
        .unwrap();
    let first = socket.next().await.unwrap().unwrap();
    match serde_json::from_str::<WsMessage>(first.to_text().unwrap()).unwrap() {
        WsMessage::Snapshot(snapshot) => assert_eq!(snapshot.viewer, "bob"),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn websocket_closes_when_session_deleted() {
    let environment = env_with(&["127.0.0.1"], Duration::from_millis(200)).await;
    let (cookie, hash) = login(&environment.state, "alice", Role::Reader, now() + 3600).await;
    let mut socket = open_ws(&environment.base, Some(&cookie), Some(PUBLIC_URL))
        .await
        .unwrap();
    let first = socket.next().await.unwrap().unwrap();
    assert!(first.is_text());

    db::delete_session(&environment.state.db, &hash)
        .await
        .unwrap();
    let ended = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match socket.next().await {
                None | Some(Err(_)) | Some(Ok(tungstenite::Message::Close(_))) => break,
                Some(Ok(_)) => {}
            }
        }
    })
    .await;
    assert!(ended.is_ok(), "socket still open 2 s after logout");
}

#[tokio::test]
async fn server_start_binds_both_listeners() {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    origin.commit("one", "main");
    let agent = fake_agent().await;
    let mut config = server_config(dir.path(), &origin, agent);
    // A free port of its own: `check_proxy` refuses the same address as `listen`.
    let free = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    config.proxy = Some(proxy_config(free, &["127.0.0.1"], dir.path()));
    let online = Arc::new(StaticOnline::new(&HashMap::new()).unwrap());

    let handle = lodestar_server::server::start(config, vec![host()], Vec::new(), online, false)
        .await
        .unwrap();
    let proxy_addr = handle.proxy_addr.expect("router entrance bound");
    assert_eq!(proxy_addr, free);
    let anonymous = me(&format!("http://{proxy_addr}"), None).await;
    assert_eq!(anonymous.role, Role::Anonymous);
    let tailnet = me(&format!("http://{}", handle.addr), None).await;
    assert_eq!(tailnet.role, Role::Admin);
    assert_eq!(tailnet.viewer.as_deref(), Some("dev"));
    handle.stop().await;
}
