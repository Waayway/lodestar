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
use serde_json::{Value, json};
use support::{FakeIssuer, Origin};
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
    env_full(trusted, session_recheck, None).await
}

/// Like [`env`], logging in against `issuer`.
async fn env_with_issuer(issuer: String) -> Env {
    env_full(&["127.0.0.1"], Duration::from_secs(60), Some(issuer)).await
}

async fn env_full(trusted: &[&str], session_recheck: Duration, issuer: Option<String>) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    origin.commit("one", "main");
    let agent = fake_agent().await;
    let mut config = server_config(dir.path(), &origin, agent);
    let mut proxy = proxy_config("127.0.0.1:0".parse().unwrap(), trusted, dir.path());
    if let Some(issuer) = issuer {
        proxy.oidc.issuer = issuer;
    }
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
async fn several_marks_count_as_none() {
    let environment = env().await;
    let base = &environment.base;
    let (cookie, _) = login(&environment.state, "bob", Role::Admin, now() + 3600).await;
    for marks in [["tailnet", "tailnet"], ["tailnet", "public"], ["public", "tailnet"]] {
        let mut request = client()
            .post(format!("{base}/api/hosts/h/apply"))
            .header("cookie", &cookie)
            .header("x-lodestar", "1");
        for mark in marks {
            request = request.header(MARK, mark);
        }
        let response = request.send().await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{marks:?}");
        assert_eq!(error_of(response).await, "only over Tailscale", "{marks:?}");
    }
    assert!(
        db::recent_audit(&environment.state.db, 100)
            .await
            .unwrap()
            .is_empty()
    );
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

// ---- Login, callback and logout (driven by hand: no redirects, no cookie store) ----

const READER_GROUP: &str = "lodestar-users@example.com";
const SESSION_ATTRIBUTES: &str = "; Path=/; Max-Age=43200; Secure; HttpOnly; SameSite=Lax";

fn no_redirects() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

fn set_cookies(response: &reqwest::Response) -> Vec<String> {
    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|value| value.to_str().unwrap().to_string())
        .collect()
}

/// The `Set-Cookie` that gives a session (a non-empty session value), if any.
fn session_set_cookie(response: &reqwest::Response) -> Option<String> {
    set_cookies(response).into_iter().find(|cookie| {
        cookie.starts_with("__Host-lodestar_session=") && !cookie.contains("Max-Age=0")
    })
}

fn location(response: &reqwest::Response) -> String {
    response
        .headers()
        .get("location")
        .expect("Location header")
        .to_str()
        .unwrap()
        .to_string()
}

fn query_of(url: &str) -> HashMap<String, String> {
    reqwest::Url::parse(url)
        .unwrap()
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect()
}

async fn session_rows(state: &AppState) -> usize {
    sessions::Entity::find().all(&state.db).await.unwrap().len()
}

/// A started login: what `/auth/login` handed the browser.
struct Started {
    state: String,
    nonce: String,
    login_cookie: String,
}

/// `/auth/login?next=…` → 303 to the fake (whose authorize endpoint the browser then visits,
/// so it learns the PKCE challenge).
async fn start_login(base: &str, next: &str) -> Started {
    let response = no_redirects()
        .get(format!("{base}/auth/login"))
        .query(&[("next", next)])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let authorize_url = location(&response);
    let cookies = set_cookies(&response);
    let query = query_of(&authorize_url);
    let state = query["state"].clone();
    assert_eq!(
        cookies,
        vec![format!(
            "__Host-lodestar_login={state}; Path=/; Max-Age=600; Secure; HttpOnly; SameSite=Lax"
        )]
    );
    let visit = reqwest::get(&authorize_url).await.unwrap();
    assert!(visit.status().is_success());
    Started {
        nonce: query["nonce"].clone(),
        login_cookie: format!("__Host-lodestar_login={state}"),
        state,
    }
}

async fn callback(base: &str, params: &[(&str, &str)], cookie: Option<&str>) -> reqwest::Response {
    let mut request = no_redirects()
        .get(format!("{base}/auth/callback"))
        .query(params);
    if let Some(cookie) = cookie {
        request = request.header("cookie", cookie);
    }
    request.send().await.unwrap()
}

fn issue(fake: &FakeIssuer, code: &str, user: &str, groups: &[&str], nonce: &str) {
    fake.issue(
        code,
        json!({
            "sub": format!("{user}-id"),
            "preferred_username": user,
            "nonce": nonce,
            "groups": groups,
        }),
    );
}

async fn assert_login_failed(response: reqwest::Response) {
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(session_set_cookie(&response).is_none());
    let body = response.text().await.unwrap();
    assert!(body.contains("Login failed"), "{body}");
    assert!(body.contains("Start the login again."), "{body}");
    assert!(body.contains("Try again"), "{body}");
    assert!(body.contains(r#"href="/auth/login""#), "{body}");
}

#[tokio::test]
async fn login_flow_sets_session_cookie() {
    let fake = FakeIssuer::start().await;
    let environment = env_with_issuer(fake.issuer()).await;
    let base = &environment.base;
    let started = start_login(base, "/#/services").await;
    issue(&fake, "c1", "alice", &[READER_GROUP], &started.nonce);

    let response = callback(
        base,
        &[("code", "c1"), ("state", &started.state)],
        Some(&started.login_cookie),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&response), "/#/services");
    let session = session_set_cookie(&response).expect("session cookie");
    let value = session
        .strip_prefix("__Host-lodestar_session=")
        .unwrap()
        .strip_suffix(SESSION_ATTRIBUTES)
        .unwrap_or_else(|| panic!("attributes of {session}"));
    assert_eq!(value.len(), 43);
    assert!(set_cookies(&response).contains(
        &"__Host-lodestar_login=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Lax".to_string()
    ));

    let me = me(base, Some(&format!("__Host-lodestar_session={value}"))).await;
    assert_eq!(me.viewer.as_deref(), Some("alice"));
    assert_eq!(me.role, Role::Reader);
    assert!(me.session);
    assert_eq!(session_rows(&environment.state).await, 1);
}

#[tokio::test]
async fn callback_replay_is_refused() {
    let fake = FakeIssuer::start().await;
    let environment = env_with_issuer(fake.issuer()).await;
    let base = &environment.base;
    let started = start_login(base, "/").await;
    issue(&fake, "c1", "alice", &[READER_GROUP], &started.nonce);
    let params = [("code", "c1"), ("state", started.state.as_str())];

    let first = callback(base, &params, Some(&started.login_cookie)).await;
    assert_eq!(first.status(), StatusCode::SEE_OTHER);
    let second = callback(base, &params, Some(&started.login_cookie)).await;
    assert_login_failed(second).await;
    assert_eq!(session_rows(&environment.state).await, 1);
}

#[tokio::test]
async fn callback_state_must_match_login_cookie() {
    let fake = FakeIssuer::start().await;
    let environment = env_with_issuer(fake.issuer()).await;
    let base = &environment.base;
    let started = start_login(base, "/").await;
    issue(&fake, "c1", "alice", &[READER_GROUP], &started.nonce);
    let params = [("code", "c1"), ("state", started.state.as_str())];

    for cookie in [None, Some("__Host-lodestar_login=someone-elses-state")] {
        assert_login_failed(callback(base, &params, cookie).await).await;
    }
    assert_eq!(session_rows(&environment.state).await, 0);
}

#[tokio::test]
async fn callback_unknown_state_after_restart() {
    let fake = FakeIssuer::start().await;
    let before = env_with_issuer(fake.issuer()).await;
    let started = start_login(&before.base, "/").await;
    issue(&fake, "c1", "alice", &[READER_GROUP], &started.nonce);

    // A fresh AuthState (as after a restart) has never heard of this state.
    let after = env_with_issuer(fake.issuer()).await;
    let response = callback(
        &after.base,
        &[("code", "c1"), ("state", &started.state)],
        Some(&started.login_cookie),
    )
    .await;
    assert_login_failed(response).await;
    assert_eq!(session_rows(&after.state).await, 0);
}

#[tokio::test]
async fn no_group_means_no_access_and_no_session() {
    let fake = FakeIssuer::start().await;
    let environment = env_with_issuer(fake.issuer()).await;
    let base = &environment.base;
    let started = start_login(base, "/").await;
    issue(&fake, "c1", "carol", &[], &started.nonce);

    let response = callback(
        base,
        &[("code", "c1"), ("state", &started.state)],
        Some(&started.login_cookie),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(session_set_cookie(&response).is_none());
    let body = response.text().await.unwrap();
    assert!(body.contains("No access"), "{body}");
    assert!(
        body.contains("Your account is not in a group that may use Lodestar."),
        "{body}"
    );
    assert!(body.contains("Try again"), "{body}");
    assert_eq!(session_rows(&environment.state).await, 0);
}

#[tokio::test]
async fn kanidm_error_param_is_no_access() {
    let fake = FakeIssuer::start().await;
    let environment = env_with_issuer(fake.issuer()).await;
    let base = &environment.base;
    let started = start_login(base, "/").await;

    let response = callback(
        base,
        &[("error", "access_denied"), ("state", &started.state)],
        Some(&started.login_cookie),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(session_set_cookie(&response).is_none());
    assert!(response.text().await.unwrap().contains("No access"));
    assert_eq!(session_rows(&environment.state).await, 0);
}

#[tokio::test]
async fn issuer_down_shows_error_page() {
    let closed = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let environment = env_with_issuer(format!("http://{closed}")).await;
    let base = &environment.base;

    let response = no_redirects()
        .get(format!("{base}/auth/login"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert!(set_cookies(&response).is_empty());
    let body = response.text().await.unwrap();
    assert!(body.contains("Can't reach the login service"), "{body}");
    assert!(body.contains("Try again in a moment."), "{body}");
    assert!(body.contains(r#"href="/auth/login""#), "{body}");

    let feed = client()
        .get(format!("{base}/api/public/services"))
        .send()
        .await
        .unwrap();
    assert_eq!(feed.status(), StatusCode::OK);
}

#[tokio::test]
async fn next_is_sanitised() {
    let fake = FakeIssuer::start().await;
    let environment = env_with_issuer(fake.issuer()).await;
    let base = &environment.base;
    let started = start_login(base, "//evil.example.com").await;
    issue(&fake, "c1", "bob", &["admins@example.com"], &started.nonce);

    let response = callback(
        base,
        &[("code", "c1"), ("state", &started.state)],
        Some(&started.login_cookie),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&response), "/");
}

#[tokio::test]
async fn logout_needs_csrf_header_and_deletes_session() {
    let environment = env().await;
    let base = &environment.base;
    let (cookie, hash) = login(&environment.state, "alice", Role::Reader, now() + 3600).await;

    let refused = client()
        .post(format!("{base}/auth/logout"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    assert_eq!(me(base, Some(&cookie)).await.role, Role::Reader);

    let response = client()
        .post(format!("{base}/auth/logout"))
        .header("cookie", &cookie)
        .header("x-lodestar", "1")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        set_cookies(&response),
        vec!["__Host-lodestar_session=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Lax"]
    );
    assert!(
        sessions::Entity::find_by_id(hash)
            .one(&environment.state.db)
            .await
            .unwrap()
            .is_none()
    );
    let me = me(base, Some(&cookie)).await;
    assert_eq!(me.role, Role::Anonymous);
    assert!(!me.session);
}
