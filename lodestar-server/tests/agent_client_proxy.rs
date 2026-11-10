//! The agent client ignores proxy settings from the environment, so the bearer token never
//! goes to a proxy. Its own test binary: it sets process-wide environment variables.

mod support;

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::http::StatusCode;
use axum::middleware;
use axum::routing::post;
use lodestar_proto::ApplyRequest;
use lodestar_server::agent_client::{AgentClient, AgentEndpoint};
use support::{agent_token_files, require_agent_token};

/// A "proxy" that counts the connections it gets and answers every one with a 502.
async fn fake_proxy(connections: Arc<AtomicUsize>) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            connections.fetch_add(1, Ordering::SeqCst);
            use tokio::io::AsyncWriteExt;
            let _ = stream
                .write_all(b"HTTP/1.1 502 Bad Gateway\r\ncontent-length: 0\r\n\r\n")
                .await;
        }
    });
    addr
}

#[tokio::test]
async fn proxy_environment_variables_are_ignored() {
    let connections = Arc::new(AtomicUsize::new(0));
    let proxy = format!("http://{}", fake_proxy(connections.clone()).await);
    // SAFETY: the only test in this binary; set before any client exists.
    unsafe {
        for name in [
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
        ] {
            std::env::set_var(name, &proxy);
        }
        std::env::remove_var("NO_PROXY");
        std::env::remove_var("no_proxy");
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let agent_addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/apply", post(|| async { StatusCode::ACCEPTED }))
                .layer(middleware::from_fn(require_agent_token)),
        )
        .await
        .unwrap()
    });

    let dir = tempfile::tempdir().unwrap();
    let client = AgentClient::new(agent_token_files(dir.path(), ["box"]), ["box"]).unwrap();
    let endpoint = AgentEndpoint {
        host: "box".into(),
        addr: agent_addr.to_string(),
    };
    let request = ApplyRequest {
        apply_id: "a1".into(),
        commit: "0".repeat(40),
    };
    let status = client.apply(&endpoint, &request).await.unwrap();
    assert_eq!(connections.load(Ordering::SeqCst), 0, "the proxy was used");
    assert_eq!(status, StatusCode::ACCEPTED);
}
