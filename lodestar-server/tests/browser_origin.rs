//! Browser attack hazards on the tailnet listener: DNS rebinding (the `Host` must be one of
//! ours), cross-origin requests and WebSocket reads (the `Origin`, when sent, must be the
//! request's own), and the anti-framing / no-sniff headers on both listeners.

mod support;

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Request, StatusCode, header};
use futures::StreamExt;
use lodestar_server::config::ServerConfig;
use lodestar_server::db;
use lodestar_server::mirror::Mirror;
use lodestar_server::state::AppState;
use lodestar_server::tailscale::{Identity, OnlineProvider, PeerInfo};
use support::Origin;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tower::ServiceExt;

/// Every peer is `alice@example.com`, who is allowed in.
struct Alice;

#[async_trait::async_trait]
impl OnlineProvider for Alice {
    async fn peers(&self) -> HashMap<String, PeerInfo> {
        HashMap::new()
    }
    async fn whois(&self, _ip: IpAddr) -> Option<Identity> {
        Some(Identity {
            login: Some("alice@example.com".into()),
            tags: vec![],
            node: "laptop".into(),
        })
    }
}

struct Env {
    state: AppState,
    _dir: tempfile::TempDir,
}

/// A tailnet listener on `listen` with Tailscale access control and `allowed_hosts`.
async fn env(listen: &str, allowed_hosts: &[&str]) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    origin.commit("one", "main");
    let config: ServerConfig = serde_json::from_value(serde_json::json!({
        "listen": listen,
        "agent_port": 7461,
        "hosts_file": dir.path().join("hosts.json"),
        "repo_url": origin.bare,
        "push_url": origin.bare,
        "state_dir": dir.path().join("state"),
        "access": {"kind": "tailscale", "allowed_users": ["alice@example.com"]},
        "online": {"kind": "static", "addresses": {}},
        "allowed_hosts": allowed_hosts,
    }))
    .unwrap();
    let connection = db::connect(&config.state_dir).await.unwrap();
    let mirror = Arc::new(Mirror::open_or_init(&config).await.unwrap());
    let state = AppState::new(
        Arc::new(config),
        connection,
        mirror,
        Arc::new(Alice),
        vec![],
    )
    .unwrap();
    Env { state, _dir: dir }
}

const LISTEN: &str = "100.64.0.1:8080";
const NAMES: [&str; 2] = ["manager", "manager.tail0000.ts.net"];

async fn tailnet() -> (Env, Router) {
    let environment = env(LISTEN, &NAMES).await;
    let app = lodestar_server::api::app(environment.state.clone())
        .layer(MockConnectInfo(SocketAddr::from(([100, 64, 0, 9], 1234))));
    (environment, app)
}

/// Sends `request` (headers as given, each pair appended) and returns the response.
async fn send(
    app: &Router,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
) -> axum::response::Response {
    let mut request = Request::builder().method(method).uri(path);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn status(app: &Router, method: &str, path: &str, headers: &[(&str, &str)]) -> StatusCode {
    send(app, method, path, headers).await.status()
}

/// Headers of a WebSocket upgrade (the oneshot can't upgrade: a request the gates let through
/// ends in 426 from the extractor, one they refuse in 403).
const UPGRADE: [(&str, &str); 4] = [
    ("connection", "upgrade"),
    ("upgrade", "websocket"),
    ("sec-websocket-version", "13"),
    ("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ=="),
];

#[tokio::test]
async fn known_hosts_get_in() {
    let (_environment, app) = tailnet().await;
    for host in [
        "100.64.0.1:8080",
        "100.64.0.1",
        "manager:8080",
        "MANAGER:8080",
        "manager.:8080",
        "manager.tail0000.ts.net:8080",
        "Manager.Tail0000.TS.net:8080",
    ] {
        assert_eq!(
            status(&app, "GET", "/api/me", &[("host", host)]).await,
            StatusCode::OK,
            "{host}"
        );
    }
}

#[tokio::test]
async fn rebinding_and_malformed_hosts_are_refused() {
    let (_environment, app) = tailnet().await;
    for host in [
        "evil.example.com:8080",
        "evil.example.com",
        "manager.evil.example.com:8080",
        "100.64.0.2:8080",
        "100.64.0.1.nip.io:8080",
        "localhost:8080",
        "127.0.0.1:8080",
        "",
        "manager:",
        "manager:8080:8080",
        "manager:80a",
        "manager:99999",
        "manager:+80",
        "alice@manager:8080",
        "manager:8080/path",
        "manager 8080",
        "manager,evil.example.com",
        "[100.64.0.1]:8080",
        "-manager:8080",
    ] {
        assert_eq!(
            status(&app, "GET", "/api/me", &[("host", host)]).await,
            StatusCode::FORBIDDEN,
            "{host:?}"
        );
    }
    // No Host at all, and two of them (even when both are ours).
    assert_eq!(
        status(&app, "GET", "/api/me", &[]).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status(
            &app,
            "GET",
            "/api/me",
            &[("host", "manager:8080"), ("host", "evil.example.com:8080")]
        )
        .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status(
            &app,
            "GET",
            "/api/me",
            &[("host", "manager:8080"), ("host", "manager:8080")]
        )
        .await,
        StatusCode::FORBIDDEN
    );
    // The static files and the WebSocket are behind the same gate.
    for path in ["/", "/index.html", "/api/ws"] {
        assert_eq!(
            status(&app, "GET", path, &[("host", "evil.example.com:8080")]).await,
            StatusCode::FORBIDDEN,
            "{path}"
        );
    }
}

#[tokio::test]
async fn absolute_form_authority_must_be_ours_too() {
    let (_environment, app) = tailnet().await;
    assert_eq!(
        status(
            &app,
            "GET",
            "http://evil.example.com:8080/api/me",
            &[("host", "manager:8080")]
        )
        .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status(
            &app,
            "GET",
            "http://manager:8080/api/me",
            &[("host", "manager:8080")]
        )
        .await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn ipv6_listen_address_gets_in_bracketed() {
    let environment = env("[fd7a:115c:a1e0::1]:8080", &NAMES).await;
    let app = lodestar_server::api::app(environment.state.clone())
        .layer(MockConnectInfo(SocketAddr::from(([100, 64, 0, 9], 1234))));
    for host in [
        "[fd7a:115c:a1e0::1]:8080",
        "[FD7A:115C:A1E0:0:0:0:0:1]:8080",
        "[fd7a:115c:a1e0::1]",
    ] {
        assert_eq!(
            status(&app, "GET", "/api/me", &[("host", host)]).await,
            StatusCode::OK,
            "{host}"
        );
    }
    for host in [
        "fd7a:115c:a1e0::1",
        "fd7a:115c:a1e0::1:8080",
        "[fd7a:115c:a1e0::2]:8080",
        "[fd7a:115c:a1e0::1%eth0]:8080",
        "[fd7a:115c:a1e0::1]8080",
    ] {
        assert_eq!(
            status(&app, "GET", "/api/me", &[("host", host)]).await,
            StatusCode::FORBIDDEN,
            "{host}"
        );
    }
    let origin = [
        ("host", "[fd7a:115c:a1e0::1]:8080"),
        ("origin", "http://[fd7a:115c:a1e0:0::1]:8080"),
        ("x-lodestar", "1"),
    ];
    assert_eq!(
        status(&app, "POST", "/api/hosts/ghost/apply", &origin).await,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn post_origin_must_be_the_request_own() {
    let (_environment, app) = tailnet().await;
    let path = "/api/hosts/ghost/apply";
    // Passing every gate ends in the handler's 404 for the unknown host.
    let passes = [
        // The CLI: no Origin.
        ("manager:8080", None),
        ("manager:8080", Some("http://manager:8080")),
        ("MANAGER:8080", Some("http://manager:8080")),
        (
            "manager.tail0000.ts.net:8080",
            Some("http://manager.tail0000.ts.net:8080"),
        ),
        ("100.64.0.1:8080", Some("http://100.64.0.1:8080")),
        ("manager", Some("http://manager")),
        ("manager:80", Some("http://manager")),
        ("manager", Some("http://manager:80")),
    ];
    for (host, origin) in passes {
        let mut headers = vec![("host", host), ("x-lodestar", "1")];
        if let Some(origin) = origin {
            headers.push(("origin", origin));
        }
        assert_eq!(
            status(&app, "POST", path, &headers).await,
            StatusCode::NOT_FOUND,
            "{host} {origin:?}"
        );
    }
    let refused = [
        "http://evil.example.com:8080",
        "null",
        "",
        "http://manager:8081",
        "https://manager:8080",
        "http://manager.tail0000.ts.net:8080",
        "http://100.64.0.1:8080",
        "http://manager:8080/",
        "http://manager:8080/api",
        "http://alice@manager:8080",
        "manager:8080",
        "http://manager:8080 http://evil.example.com",
        "ws://manager:8080",
    ];
    for origin in refused {
        let headers = [
            ("host", "manager:8080"),
            ("x-lodestar", "1"),
            ("origin", origin),
        ];
        assert_eq!(
            status(&app, "POST", path, &headers).await,
            StatusCode::FORBIDDEN,
            "{origin:?}"
        );
    }
    // Two Origins, even when the first one is right.
    let headers = [
        ("host", "manager:8080"),
        ("x-lodestar", "1"),
        ("origin", "http://manager:8080"),
        ("origin", "http://evil.example.com"),
    ];
    assert_eq!(
        status(&app, "POST", path, &headers).await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn websocket_origin_must_be_the_request_own() {
    let (_environment, app) = tailnet().await;
    let with = |origin: Option<&'static str>| {
        let mut headers = UPGRADE.to_vec();
        headers.push(("host", "manager:8080"));
        if let Some(origin) = origin {
            headers.push(("origin", origin));
        }
        headers
    };
    for origin in [
        Some("http://evil.example.com"),
        Some("http://evil.example.com:8080"),
        Some("null"),
        Some("http://manager:8081"),
        Some("https://manager:8080"),
    ] {
        assert_eq!(
            status(&app, "GET", "/api/ws", &with(origin)).await,
            StatusCode::FORBIDDEN,
            "{origin:?}"
        );
    }
    // Not a browser (no Origin) or the dashboard's own page: through to the upgrade.
    for origin in [None, Some("http://manager:8080")] {
        assert_ne!(
            status(&app, "GET", "/api/ws", &with(origin)).await,
            StatusCode::FORBIDDEN,
            "{origin:?}"
        );
    }
    let mut two = with(Some("http://manager:8080"));
    two.push(("origin", "http://evil.example.com"));
    assert_eq!(
        status(&app, "GET", "/api/ws", &two).await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn websocket_end_to_end_from_the_dashboard_origin() {
    let environment = env("127.0.0.1:0", &[]).await;
    let app = lodestar_server::api::app(environment.state.clone());
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
    let open = |origin: String| async move {
        let mut request = format!("ws://{addr}/api/ws").into_client_request().unwrap();
        request
            .headers_mut()
            .insert("origin", origin.parse().unwrap());
        tokio_tungstenite::connect_async(request).await
    };
    let (mut socket, _) = open(format!("http://{addr}")).await.unwrap();
    assert!(socket.next().await.unwrap().unwrap().is_text());
    match open("http://evil.example.com".into()).await {
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
            assert_eq!(response.status(), StatusCode::FORBIDDEN)
        }
        other => panic!("expected 403, got {:?}", other.map(|_| ())),
    }
}

#[tokio::test]
async fn loopback_listener_also_answers_to_localhost() {
    // The demo and the Vite dev server (`localhost:5173` proxying with its own Host).
    let environment = env("127.0.0.1:8080", &[]).await;
    let app = lodestar_server::api::app(environment.state.clone())
        .layer(MockConnectInfo(SocketAddr::from(([127, 0, 0, 1], 1234))));
    for host in ["127.0.0.1:8080", "localhost:8080", "localhost:5173"] {
        assert_eq!(
            status(&app, "GET", "/api/me", &[("host", host)]).await,
            StatusCode::OK,
            "{host}"
        );
    }
    assert_eq!(
        status(&app, "GET", "/api/me", &[("host", "evil.example.com:8080")]).await,
        StatusCode::FORBIDDEN
    );
    let headers = [
        ("host", "localhost:5173"),
        ("origin", "http://localhost:5173"),
        ("x-lodestar", "1"),
    ];
    assert_eq!(
        status(&app, "POST", "/api/hosts/ghost/apply", &headers).await,
        StatusCode::NOT_FOUND
    );
}

fn assert_hardened(response: &axum::response::Response, label: &str) {
    let headers = response.headers();
    assert_eq!(
        headers.get("x-frame-options").map(|v| v.as_bytes()),
        Some(&b"DENY"[..]),
        "{label}"
    );
    assert_eq!(
        headers.get("content-security-policy").map(|v| v.as_bytes()),
        Some(&b"frame-ancestors 'none'"[..]),
        "{label}"
    );
    assert_eq!(
        headers.get("x-content-type-options").map(|v| v.as_bytes()),
        Some(&b"nosniff"[..]),
        "{label}"
    );
}

#[tokio::test]
async fn tailnet_responses_refuse_framing_and_sniffing() {
    let (_environment, app) = tailnet().await;
    let ours = [("host", "manager:8080")];
    for path in [
        "/",
        "/index.html",
        "/some/client/route",
        "/api/me",
        "/api/nope",
    ] {
        let response = send(&app, "GET", path, &ours).await;
        assert_hardened(&response, path);
    }
    let index = send(&app, "GET", "/", &ours).await;
    assert_eq!(index.status(), StatusCode::OK);
    assert!(
        index.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/html")
    );
    // Refusals too.
    let refused = send(&app, "GET", "/", &[("host", "evil.example.com")]).await;
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    assert_hardened(&refused, "refused host");
}
