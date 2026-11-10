//! The agent client against fake agents: it follows no redirect and reads no more than
//! `MAX_RESPONSE_BYTES` of an answer.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use axum::Router;
use axum::body::{Body, Bytes};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use lodestar_proto::ApplyRequest;
use lodestar_server::agent_client::{AgentClient, MAX_RESPONSE_BYTES};

async fn serve(app: Router) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

/// A server that counts every request it gets.
async fn counting_target() -> (SocketAddr, Arc<AtomicU32>) {
    let hits = Arc::new(AtomicU32::new(0));
    let counter = hits.clone();
    let app = Router::new().fallback(move || {
        let counter = counter.clone();
        async move {
            counter.fetch_add(1, Ordering::SeqCst);
            StatusCode::ACCEPTED
        }
    });
    (serve(app).await, hits)
}

fn redirect_to(target: SocketAddr, path: &str) -> Response {
    (
        StatusCode::TEMPORARY_REDIRECT,
        [(header::LOCATION, format!("http://{target}{path}"))],
    )
        .into_response()
}

#[tokio::test]
async fn redirects_are_not_followed() {
    let (target, hits) = counting_target().await;
    let app = Router::new()
        .route(
            "/status",
            get(move || async move { redirect_to(target, "/status") }),
        )
        .route(
            "/apply/log",
            get(move || async move { redirect_to(target, "/apply/log") }),
        )
        .route(
            "/apply",
            post(move || async move { redirect_to(target, "/apply") }),
        );
    let agent = serve(app).await.to_string();
    let client = AgentClient::new().unwrap();

    assert!(client.status(&agent).await.is_err());
    assert!(client.log(&agent, "a1", 0).await.is_err());
    let request = ApplyRequest {
        apply_id: "a1".into(),
        commit: "1".repeat(40),
    };
    let code = client.apply(&agent, &request).await.unwrap();
    assert_eq!(code, StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

/// An answer that never ends (and claims no length).
fn endless() -> Response {
    let chunk = Bytes::from(vec![b' '; 64 * 1024]);
    let stream = futures::stream::repeat_with(move || Ok::<_, std::io::Error>(chunk.clone()));
    (
        [(header::CONTENT_TYPE, "application/json")],
        Body::from_stream(stream),
    )
        .into_response()
}

/// A JSON log chunk just over the cap.
fn oversized_log() -> Response {
    let data = "x".repeat(MAX_RESPONSE_BYTES);
    let body = format!(r#"{{"apply_id":"a1","offset":0,"data":"{data}","next":1,"eof":true}}"#);
    ([(header::CONTENT_TYPE, "application/json")], body).into_response()
}

#[tokio::test]
async fn endless_answers_are_cut_off() {
    let app = Router::new()
        .route("/status", get(|| async { endless() }))
        .route("/apply/log", get(|| async { endless() }))
        .route("/activations", get(|| async { endless() }));
    let agent = serve(app).await.to_string();
    let client = AgentClient::new().unwrap();
    let error = client.status(&agent).await.unwrap_err();
    assert!(format!("{error:#}").contains("too large"), "{error:#}");
    assert!(client.log(&agent, "a1", 0).await.is_err());
    assert!(client.activations(&agent, 0).await.is_err());
}

#[tokio::test]
async fn answers_over_the_cap_are_refused() {
    let app = Router::new().route("/apply/log", get(|| async { oversized_log() }));
    let agent = serve(app).await.to_string();
    let error = AgentClient::new()
        .unwrap()
        .log(&agent, "a1", 0)
        .await
        .unwrap_err();
    assert!(format!("{error:#}").contains("too large"), "{error:#}");
}

#[tokio::test]
async fn answers_under_the_cap_are_read() {
    let app = Router::new().route(
        "/apply/log",
        get(|| async {
            let data = "x".repeat(1024 * 1024);
            let body = format!(
                r#"{{"apply_id":"a1","offset":0,"data":"{data}","next":1048576,"eof":true}}"#
            );
            ([(header::CONTENT_TYPE, "application/json")], body)
        }),
    );
    let agent = serve(app).await.to_string();
    let chunk = AgentClient::new()
        .unwrap()
        .log(&agent, "a1", 0)
        .await
        .unwrap();
    assert_eq!(chunk.data.len(), 1024 * 1024);
}
