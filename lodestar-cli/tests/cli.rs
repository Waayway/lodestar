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

/// `rebuild --wait` survives a restart of the server mid-wait (the apply switched the host
/// that runs it): it retries quietly and still reports the final result.
#[tokio::test]
async fn rebuild_wait_survives_a_server_restart() {
    // Slow enough that the apply is still running across the restart.
    let mut d = demo::start("127.0.0.1:0".parse().unwrap(), 2.0)
        .await
        .unwrap();
    let mut c = Client::new(&format!("http://{}", d.addr));
    c.poll = Duration::from_millis(100);
    c.retry = (Duration::from_millis(200), Duration::from_millis(500));
    tokio::time::sleep(Duration::from_secs(3)).await; // the first poll reached every agent
    let wait = tokio::spawn(async move {
        let mut out = Vec::new();
        let cmd = Command::Rebuild {
            host: Some("desktop".into()),
            channel: None,
            wait: true,
            timeout: 60,
        };
        let code = run(&c, cmd, &mut out).await.unwrap();
        (code, String::from_utf8(out).unwrap())
    });
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!wait.is_finished(), "the apply finished before the restart");
    d.restart(Duration::from_millis(1500)).await.unwrap();
    let (code, out) = wait.await.unwrap();
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("desktop: ok"), "{out}");
    assert_eq!(out.matches("server restarting").count(), 1, "{out}");
}

#[tokio::test]
async fn rebuild_wait_stops_on_a_non_transient_error() {
    let (_d, c) = start().await;
    // 404 from the server is final, not retried.
    let err = run(&c, rebuild("nope", true), &mut Vec::new())
        .await
        .unwrap_err();
    assert!(!lodestar_cli::is_transient(&err), "{err:#}");
    let dead = Client::new("http://127.0.0.1:1");
    let err = run(&dead, Command::Status, &mut Vec::new())
        .await
        .unwrap_err();
    assert!(lodestar_cli::is_transient(&err), "{err:#}");
}
