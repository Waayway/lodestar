//! Runs the CLI library against the real server in demo mode.

use lodestar_cli::{Client, Command, run};
use lodestar_server::demo::{self, DemoHandle};
use std::time::Duration;

async fn start() -> (DemoHandle, Client) {
    let d = demo::start("127.0.0.1:0".parse().unwrap(), 20.0)
        .await
        .unwrap();
    let mut c = Client::new(&format!("http://{}", d.addr));
    c.poll = Duration::from_millis(100);
    (d, c)
}

async fn go(c: &Client, cmd: Command) -> (i32, String) {
    let mut out = Vec::new();
    let code = run(c, cmd, &mut out).await.unwrap();
    (code, String::from_utf8(out).unwrap())
}

fn rebuild(host: &str, wait: bool) -> Command {
    Command::Rebuild {
        host: Some(host.into()),
        channel: None,
        wait,
        timeout: 30,
    }
}

#[tokio::test]
async fn status_lists_seven_hosts() {
    let (_d, c) = start().await;
    let (code, out) = go(&c, Command::Status).await;
    assert_eq!(code, 0);
    assert_eq!(out.lines().count(), 8, "{out}"); // header + 7
    assert!(out.contains("desktop") && out.contains("laptop-1"), "{out}");
}

#[tokio::test]
async fn rebuild_desktop_wait_succeeds() {
    let (_d, c) = start().await;
    // Wait until the agent has been polled so the apply reaches it.
    tokio::time::sleep(Duration::from_secs(1)).await;
    let (code, out) = go(&c, rebuild("desktop", true)).await;
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("apply_id") && out.contains("desktop: ok"),
        "{out}"
    );
}

#[tokio::test]
async fn rebuild_laptop_1_wait_reports_skipped_dirty() {
    let (_d, c) = start().await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let (code, out) = go(&c, rebuild("laptop-1", true)).await;
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("skipped (dirty)"), "{out}");
}

#[tokio::test]
async fn posts_carry_x_lodestar_and_errors_are_clear() {
    let (_d, c) = start().await;
    // The demo server rejects a POST without X-Lodestar, so success proves the header is sent.
    let (code, _) = go(&c, rebuild("desktop", false)).await;
    assert_eq!(code, 0);
    let err = run(&c, rebuild("nope", false), &mut Vec::new())
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("404"), "{err:#}");
    let dead = Client::new("http://127.0.0.1:1");
    let err = run(&dead, Command::Status, &mut Vec::new())
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("cannot reach"), "{err:#}");
}

#[test]
fn server_from_config_file() {
    use lodestar_cli::server_from_config;
    assert_eq!(
        server_from_config("# lodestar\nserver = \"http://manager:8080\"\n").as_deref(),
        Some("http://manager:8080")
    );
    assert_eq!(
        server_from_config("server='manager:8080' # x").as_deref(),
        Some("manager:8080")
    );
    assert_eq!(server_from_config("[other]\nserver = \"x\""), None);
    assert_eq!(server_from_config("servers = \"x\""), None);
    assert_eq!(server_from_config(""), None);
}

#[test]
fn explicit_server_wins() {
    assert_eq!(
        lodestar_cli::resolve_server(Some("http://a:1".into())).unwrap(),
        "http://a:1"
    );
}

#[tokio::test]
async fn promote_reports_the_configured_pair() {
    let (_d, c) = start().await;
    let (code, out) = go(&c, Command::Promote { yes: false }).await;
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("unstable -> stable"), "{out}");
    assert!(out.contains("move stable"), "{out}");
}

/// Starts a slow demo (applies take seconds) and `rebuild desktop --wait` in the background,
/// and returns once the apply is running on the agent.
async fn waiting_rebuild(
    timeout: u64,
) -> (
    DemoHandle,
    tokio::task::JoinHandle<(anyhow::Result<i32>, String)>,
) {
    let d = demo::start("127.0.0.1:0".parse().unwrap(), 2.0)
        .await
        .unwrap();
    let mut c = Client::new(&format!("http://{}", d.addr));
    c.poll = Duration::from_millis(100);
    c.retry = (Duration::from_millis(200), Duration::from_millis(500));
    let probe = Client::new(&format!("http://{}", d.addr));
    // The first poll must have reached the agent, or the apply only starts on a re-send.
    wait_until(&probe, |h| h.checkout.is_some()).await;
    let wait = tokio::spawn(async move {
        let mut out = Vec::new();
        let cmd = Command::Rebuild {
            host: Some("desktop".into()),
            channel: None,
            wait: true,
            timeout,
        };
        let r = run(&c, cmd, &mut out).await;
        (r, String::from_utf8(out).unwrap())
    });
    wait_until(&probe, |h| {
        h.last_apply
            .as_ref()
            .is_some_and(|a| a.state == lodestar_proto::ApplyState::Running)
    })
    .await;
    assert!(!wait.is_finished(), "the CLI returned before the apply ran");
    (d, wait)
}

/// Polls the snapshot until `desktop` satisfies `ok` (at most 20 s).
async fn wait_until(c: &Client, ok: impl Fn(&lodestar_proto::HostView) -> bool) {
    for _ in 0..200 {
        let snap = c.snapshot().await.unwrap();
        if snap.hosts.iter().any(|h| h.name == "desktop" && ok(h)) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("desktop never reached the expected state");
}

/// `rebuild --wait` survives a restart of the server mid-wait (the apply switched the host
/// that runs it): it retries quietly and still reports the final result.
#[tokio::test]
async fn rebuild_wait_survives_a_server_restart() {
    let (mut d, wait) = waiting_rebuild(60).await;
    d.restart(Duration::from_millis(1500)).await.unwrap();
    let (r, out) = wait.await.unwrap();
    assert_eq!(r.unwrap(), 0, "{out}");
    assert!(out.contains("desktop: ok"), "{out}");
    assert_eq!(out.matches("server restarting").count(), 1, "{out}");
}

/// Replaces the stopped demo server with one that answers every request with `status`
/// and `body`.
async fn impostor(addr: std::net::SocketAddr, status: u16, body: &'static str) {
    let app = axum::Router::new()
        .fallback(move || async move { (axum::http::StatusCode::from_u16(status).unwrap(), body) });
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
}

/// During the wait, an answer the server decided on (403, or a body that is not the
/// expected JSON) ends the wait at once instead of being retried until the timeout.
#[tokio::test]
async fn rebuild_wait_stops_on_a_non_transient_error() {
    for (status, body, expect) in [
        (403, r#"{"error":"forbidden"}"#, "not allowed"),
        (200, "not json", "decoding response"),
    ] {
        let (mut d, wait) = waiting_rebuild(60).await;
        d.stop_server().await;
        impostor(d.addr, status, body).await;
        let started = std::time::Instant::now();
        let (r, out) = tokio::time::timeout(Duration::from_secs(10), wait)
            .await
            .expect("the wait did not stop")
            .unwrap();
        let err = r.expect_err(&out);
        assert!(format!("{err:#}").contains(expect), "{err:#}");
        assert!(!lodestar_cli::is_transient(&err), "{err:#}");
        assert!(!out.contains("server restarting"), "{out}");
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}

/// The wait never outlasts --timeout, even when the server accepts a request and never
/// answers it (the client's own request timeout is a minute).
#[tokio::test]
async fn rebuild_wait_respects_the_timeout_with_a_hanging_server() {
    let (mut d, wait) = waiting_rebuild(5).await;
    d.stop_server().await;
    let listener = tokio::net::TcpListener::bind(d.addr).await.unwrap();
    let app = axum::Router::new().fallback(|| async {
        tokio::time::sleep(Duration::from_secs(300)).await;
        ""
    });
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (r, out) = tokio::time::timeout(Duration::from_secs(10), wait)
        .await
        .expect("the wait overshot --timeout")
        .unwrap();
    assert_eq!(r.unwrap(), 1, "{out}");
    assert!(out.contains("timed out waiting for: desktop"), "{out}");
}

#[test]
fn transient_errors() {
    use lodestar_cli::{RequestError, is_transient};
    let t = |e: RequestError| is_transient(&anyhow::Error::new(e));
    assert!(t(RequestError::Unreachable("refused".into())));
    assert!(t(RequestError::Status(503, String::new())));
    assert!(!t(RequestError::Status(403, String::new())));
    assert!(!t(RequestError::Status(404, String::new())));
    assert!(!is_transient(&anyhow::anyhow!("decoding response")));
}
