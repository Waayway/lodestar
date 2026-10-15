//! HTTP probe of service links against an in-process axum app on 127.0.0.1:0.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use axum::Router;
use axum::http::{StatusCode, header};
use axum::routing::get;
use lodestar_proto::{DownReason, ServiceState};
use lodestar_server::config::{Link, LinkCheck};
use lodestar_server::poller::service_check::{Probers, probe};

async fn serve() -> SocketAddr {
    let app = Router::new()
        .route("/ok", get(|| async { "fine" }))
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
