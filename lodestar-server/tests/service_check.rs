//! HTTP probe of service links against an in-process axum app on 127.0.0.1:0.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use axum::Router;
use axum::http::{StatusCode, header};
use axum::routing::get;
use lodestar_proto::{DownReason, ServiceState, WsMessage};
use lodestar_server::config::{
    AccessConfig, Link, LinkCheck, ManagedHost, OnlineSource, ServerConfig,
};
use lodestar_server::db;
use lodestar_server::mirror::Mirror;
use lodestar_server::poller::service_check::{CHECK_TIMEOUT, Probers, check_round, probe};
use lodestar_server::state::{AppState, snapshot};
use lodestar_server::tailscale::StaticOnline;

static HITS: AtomicUsize = AtomicUsize::new(0);
static FLIP: AtomicUsize = AtomicUsize::new(0);

async fn serve() -> SocketAddr {
    let app = Router::new()
        .route("/ok", get(|| async { "fine" }))
        .route(
            "/counted",
            get(|| async {
                HITS.fetch_add(1, Ordering::SeqCst);
                "counted"
            }),
        )
        .route(
            "/flip",
            get(|| async {
                if FLIP.fetch_add(1, Ordering::SeqCst) < 2 {
                    StatusCode::OK
                } else {
                    StatusCode::BAD_GATEWAY
                }
            }),
        )
        .route("/login", get(|| async { StatusCode::UNAUTHORIZED }))
        .route("/bad", get(|| async { StatusCode::BAD_GATEWAY }))
        .route(
            "/redir",
            get(|| async {
                (
                    StatusCode::FOUND,
                    [(header::LOCATION, "http://10.255.255.1/")],
                )
            }),
        )
        .route(
            "/hang",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(10)).await;
                "late"
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

fn link(url: String) -> Link {
    Link {
        id: "docs".into(),
        title: "Docs".into(),
        url,
        group: "Tools".into(),
        description: None,
        icon: None,
        host: "alpha".into(),
        check: LinkCheck {
            enable: true,
            url: None,
            expect_status: vec![200, 204, 301, 302, 303, 307, 308, 401, 403],
            insecure_tls: false,
        },
    }
}

async fn closed_port_url() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    format!("http://127.0.0.1:{port}/")
}

#[tokio::test]
async fn ok_is_up() {
    let addr = serve().await;
    let out = probe(&Probers::new().unwrap(), &link(format!("http://{addr}/ok"))).await;
    assert_eq!(out.state, ServiceState::Up);
    assert!(out.latency_ms.is_some());
    assert_eq!(out.reason, None);
}

#[tokio::test]
async fn unauthorized_counts_as_up() {
    let addr = serve().await;
    let out = probe(
        &Probers::new().unwrap(),
        &link(format!("http://{addr}/login")),
    )
    .await;
    assert_eq!(out.state, ServiceState::Up);
}

#[tokio::test]
async fn bad_gateway_is_down() {
    let addr = serve().await;
    let out = probe(
        &Probers::new().unwrap(),
        &link(format!("http://{addr}/bad")),
    )
    .await;
    assert_eq!(out.state, ServiceState::Down);
    assert_eq!(out.reason, Some(DownReason::HttpStatus(502)));
}

#[tokio::test]
async fn redirect_is_not_followed() {
    let addr = serve().await;
    let started = Instant::now();
    let out = probe(
        &Probers::new().unwrap(),
        &link(format!("http://{addr}/redir")),
    )
    .await;
    assert_eq!(out.state, ServiceState::Up);
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[tokio::test]
async fn hang_times_out() {
    let addr = serve().await;
    let started = Instant::now();
    let out = probe(
        &Probers::new().unwrap(),
        &link(format!("http://{addr}/hang")),
    )
    .await;
    assert_eq!(out.state, ServiceState::Down);
    assert_eq!(out.reason, Some(DownReason::Timeout));
    assert!(started.elapsed() < Duration::from_secs(6));
}

#[tokio::test]
async fn closed_port_is_refused() {
    let out = probe(&Probers::new().unwrap(), &link(closed_port_url().await)).await;
    assert_eq!(out.state, ServiceState::Down);
    assert_eq!(out.reason, Some(DownReason::ConnectionRefused));
}

#[tokio::test]
async fn status_outside_expect_is_down() {
    let addr = serve().await;
    let mut l = link(format!("http://{addr}/ok"));
    l.check.expect_status = vec![204];
    let out = probe(&Probers::new().unwrap(), &l).await;
    assert_eq!(out.state, ServiceState::Down);
    assert_eq!(out.reason, Some(DownReason::HttpStatus(200)));
}

#[tokio::test]
async fn check_url_overrides_url() {
    let addr = serve().await;
    let mut l = link(closed_port_url().await);
    l.check.url = Some(format!("http://{addr}/ok"));
    let out = probe(&Probers::new().unwrap(), &l).await;
    assert_eq!(out.state, ServiceState::Up);
}

fn empty_origin(dir: &Path) -> PathBuf {
    let bare = dir.join("origin.git");
    let output = Command::new("git")
        .args(["init", "--bare", "--quiet", bare.to_str().unwrap()])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(output.status.success());
    bare
}

async fn state_with(links: Vec<Link>) -> (AppState, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let origin = empty_origin(dir.path());
    let addresses = HashMap::from([("alpha".to_string(), "127.0.0.1:1".to_string())]);
    let config = ServerConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        agent_port: 7461,
        hosts_file: dir.path().join("hosts.json"),
        links_file: None,
        repo_url: origin.to_str().unwrap().into(),
        push_url: origin.to_str().unwrap().into(),
        deploy_key_file: None,
        state_dir: dir.path().join("state"),
        access: AccessConfig::Disabled,
        online: OnlineSource::Static {
            addresses: addresses.clone(),
        },
        mirror_fetch_secs: 300,
        poll_secs: 60,
        tailscale_bin: PathBuf::from("tailscale"),
        promote: Some(lodestar_proto::PromoteChannels::default()),
    };
    let connection = db::connect(&config.state_dir).await.unwrap();
    let mirror = Arc::new(Mirror::open_or_init(&config).await.unwrap());
    let online = Arc::new(StaticOnline::new(&addresses).unwrap());
    let host = ManagedHost {
        name: "alpha".into(),
        system: "x86_64-linux".into(),
        user: "u".into(),
        channel: "stable".into(),
        os: "nixos".into(),
    };
    let mut state =
        AppState::new(Arc::new(config), connection, mirror, online, vec![host]).unwrap();
    state.links = Arc::new(links);
    (state, dir)
}

fn named(id: &str, group: &str, url: String) -> Link {
    let mut l = link(url);
    l.id = id.into();
    l.title = id.into();
    l.group = group.into();
    l
}

#[tokio::test]
async fn round_records_and_broadcasts_once() {
    let addr = serve().await;
    let (state, _dir) = state_with(vec![named("a", "G", format!("http://{addr}/ok"))]).await;
    let probers = Probers::new().unwrap();
    let mut rx = state.tx.subscribe();
    assert!(check_round(&state, &probers).await);
    match rx.try_recv().unwrap() {
        WsMessage::ServicesUpdated(views) => {
            assert_eq!(views.len(), 1);
            assert_eq!(views[0].check.state, ServiceState::Up);
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(!check_round(&state, &probers).await);
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn since_moves_only_on_change() {
    let addr = serve().await;
    FLIP.store(0, Ordering::SeqCst);
    // /flip answers 200 on the first two calls, then 502.
    let (state, _dir) = state_with(vec![named("a", "G", format!("http://{addr}/flip"))]).await;
    let probers = Probers::new().unwrap();
    check_round(&state, &probers).await;
    let first = state.checks.lock().unwrap()["a"].clone();
    assert_eq!(first.state, ServiceState::Up);
    tokio::time::sleep(Duration::from_millis(1100)).await;
    check_round(&state, &probers).await;
    let second = state.checks.lock().unwrap()["a"].clone();
    assert_eq!(second.state, ServiceState::Up);
    assert_eq!(second.since, first.since);
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert!(check_round(&state, &probers).await);
    let third = state.checks.lock().unwrap()["a"].clone();
    assert_eq!(third.state, ServiceState::Down);
    assert_ne!(third.since, first.since);
}

#[tokio::test]
async fn disabled_link_is_unchecked_and_not_probed() {
    let addr = serve().await;
    HITS.store(0, Ordering::SeqCst);
    let mut l = named("a", "G", format!("http://{addr}/counted"));
    l.check.enable = false;
    let (state, _dir) = state_with(vec![l]).await;
    assert!(!check_round(&state, &Probers::new().unwrap()).await);
    assert_eq!(HITS.load(Ordering::SeqCst), 0);
    let snap = snapshot(&state, "v".into()).await;
    assert_eq!(snap.services[0].check.state, ServiceState::Unchecked);
}

#[tokio::test]
async fn snapshot_lists_services_sorted() {
    let (state, _dir) = state_with(vec![named("b1", "B", "http://127.0.0.1:1/".into()), {
        let mut l = named("a1", "A", "http://127.0.0.1:1/".into());
        l.host = "nowhere".into();
        l
    }])
    .await;
    let snap = snapshot(&state, "v".into()).await;
    let ids: Vec<_> = snap.services.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["a1", "b1"]);
    assert_eq!(snap.services[0].host_online, None);
    assert!(snap.services[1].host_online.is_some());
    assert_eq!(snap.services[1].check.state, ServiceState::Pending);
}

#[tokio::test]
async fn round_with_hanging_links_is_bounded() {
    let addr = serve().await;
    let links = (0..9)
        .map(|i| named(&format!("h{i}"), "G", format!("http://{addr}/hang")))
        .collect();
    let (state, _dir) = state_with(links).await;
    let started = Instant::now();
    check_round(&state, &Probers::new().unwrap()).await;
    assert!(started.elapsed() < 2 * CHECK_TIMEOUT + Duration::from_secs(1));
}
