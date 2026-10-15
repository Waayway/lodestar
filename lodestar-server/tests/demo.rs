//! Demo mode: the real server against a simulated fleet, run at 20x speed.

use std::time::{Duration, Instant};

use lodestar_proto::{ApplyState, HostView, OnlineState, SkipReason, Snapshot};
use lodestar_server::demo::{self, DemoHandle};
use serde_json::{Value, json};

const SPEED: f64 = 20.0;

async fn start() -> DemoHandle {
    demo::start("127.0.0.1:0".parse().unwrap(), SPEED)
        .await
        .unwrap()
}

async fn snapshot(demo_handle: &DemoHandle) -> Snapshot {
    reqwest::get(format!("http://{}/api/snapshot", demo_handle.addr))
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

fn host<'a>(fleet: &'a Snapshot, name: &str) -> &'a HostView {
    fleet
        .hosts
        .iter()
        .find(|host_view| host_view.name == name)
        .unwrap()
}

fn head(fleet: &Snapshot, channel: &str) -> Option<String> {
    fleet
        .channels
        .iter()
        .find(|view| view.name == channel)
        .and_then(|view| view.head.clone())
}

/// Polls the snapshot until `pred` holds, or panics with the last snapshot after `secs`.
async fn wait_for(
    demo_handle: &DemoHandle,
    secs: u64,
    what: &str,
    predicate: impl Fn(&Snapshot) -> bool,
) -> Snapshot {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        let fleet = snapshot(demo_handle).await;
        if predicate(&fleet) {
            return fleet;
        }
        if Instant::now() > deadline {
            panic!("timed out waiting for {what}: {fleet:#?}");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// A client that sends the header the dashboard sends (`X-Lodestar: 1`, CSRF guard).
fn dashboard_client() -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("x-lodestar", reqwest::header::HeaderValue::from_static("1"));
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap()
}

async fn post(demo_handle: &DemoHandle, path: &str, body: Value) -> (u16, Value) {
    let response = dashboard_client()
        .post(format!("http://{}{path}", demo_handle.addr))
        .json(&body)
        .send()
        .await
        .unwrap();
    let code = response.status().as_u16();
    (code, response.json().await.unwrap())
}

async fn apply(demo_handle: &DemoHandle, name: &str) -> Value {
    let (code, body) = post(demo_handle, &format!("/api/hosts/{name}/apply"), json!({})).await;
    assert_eq!(code, 202, "{body}");
    body
}

fn last_state(fleet: &Snapshot, name: &str) -> Option<ApplyState> {
    host(fleet, name)
        .last_apply
        .as_ref()
        .map(|last_apply| last_apply.state.clone())
}

/// Waits until the host's agent has been polled at least once.
async fn wait_polled(demo_handle: &DemoHandle, name: &str) {
    wait_for(demo_handle, 10, &format!("{name} polled"), |fleet| {
        host(fleet, name).online == OnlineState::On
    })
    .await;
}

#[tokio::test]
async fn demo_starts_and_serves_snapshot() {
    let demo_handle = start().await;
    let fleet = wait_for(&demo_handle, 10, "laptop-1 checkout", |fleet| {
        host(fleet, "laptop-1").checkout.is_some()
    })
    .await;
    assert!(fleet.demo);
    assert_eq!(fleet.hosts.len(), 7);
    let mut names: Vec<_> = fleet
        .channels
        .iter()
        .map(|channel| channel.name.as_str())
        .collect();
    names.sort();
    assert_eq!(names, ["stable", "unstable"]);
    assert!(head(&fleet, "unstable").is_some());
    assert!(head(&fleet, "stable").is_some());
    assert!(host(&fleet, "laptop-1").checkout.as_ref().unwrap().dirty);
}

#[tokio::test]
async fn demo_apply_desktop_reaches_ok() {
    let demo_handle = start().await;
    wait_polled(&demo_handle, "desktop").await;
    apply(&demo_handle, "desktop").await;
    let fleet = wait_for(&demo_handle, 10, "desktop ok", |fleet| {
        last_state(fleet, "desktop") == Some(ApplyState::Ok)
    })
    .await;
    let host_view = host(&fleet, "desktop");
    assert_eq!(host_view.running, head(&fleet, "unstable"));
    assert_eq!(host_view.behind, Some(0));
}

#[tokio::test]
async fn demo_apply_laptop_1_skips_dirty() {
    let demo_handle = start().await;
    wait_polled(&demo_handle, "laptop-1").await;
    apply(&demo_handle, "laptop-1").await;
    let fleet = wait_for(&demo_handle, 10, "laptop-1 skipped", |fleet| {
        last_state(fleet, "laptop-1") == Some(ApplyState::Skipped)
    })
    .await;
    let last_apply = host(&fleet, "laptop-1").last_apply.clone().unwrap();
    assert_eq!(last_apply.skip_reason, Some(SkipReason::Dirty));
}

#[tokio::test]
async fn demo_media_first_apply_fails() {
    let demo_handle = start().await;
    wait_polled(&demo_handle, "media").await;
    apply(&demo_handle, "media").await;
    let fleet = wait_for(&demo_handle, 10, "media failed", |fleet| {
        last_state(fleet, "media") == Some(ApplyState::Failed)
    })
    .await;
    let last_apply = host(&fleet, "media").last_apply.clone().unwrap();
    assert!(
        last_apply.exit_code.is_some_and(|code| code != 0),
        "{last_apply:?}"
    );
}

#[tokio::test]
async fn demo_laptop_2_resend_after_online() {
    let demo_handle = start().await;
    let fleet = snapshot(&demo_handle).await;
    assert_eq!(host(&fleet, "laptop-2").online, OnlineState::Off);
    let body = apply(&demo_handle, "laptop-2").await;
    assert_eq!(body["agent_status"], "unreachable", "{body}");
    let fleet = wait_for(&demo_handle, 20, "laptop-2 ok", |fleet| {
        last_state(fleet, "laptop-2") == Some(ApplyState::Ok)
    })
    .await;
    let last_apply = host(&fleet, "laptop-2").last_apply.clone().unwrap();
    assert_eq!(
        Some(last_apply.apply_id),
        body["apply_id"].as_str().map(String::from)
    );
    assert_eq!(host(&fleet, "laptop-2").running, Some(last_apply.commit));
}

#[tokio::test]
async fn demo_promote_moves_stable() {
    let demo_handle = start().await;
    let fleet = snapshot(&demo_handle).await;
    let candidate = fleet.promote_candidate.clone().expect("promote candidate");
    let (code, body) = post(&demo_handle, "/api/promote", json!({ "commit": candidate })).await;
    assert_eq!(code, 200, "{body}");
    let fleet = snapshot(&demo_handle).await;
    assert_eq!(head(&fleet, "stable"), Some(candidate));
}

#[tokio::test]
async fn demo_has_services_in_every_state() {
    use lodestar_proto::ServiceState;
    let demo_handle = start().await;
    let fleet = wait_for(&demo_handle, 10, "services in every state", |fleet| {
        let has = |state: ServiceState| fleet.services.iter().any(|s| s.check.state == state);
        has(ServiceState::Up)
            && has(ServiceState::Down)
            && has(ServiceState::Unchecked)
            && fleet
                .services
                .iter()
                .any(|s| s.id == "backup-ui" && s.check.state == ServiceState::Down)
            && fleet
                .services
                .iter()
                .any(|s| s.host_online == Some(OnlineState::Off))
    })
    .await;
    let stuck = fleet.services.iter().find(|s| s.id == "backup-ui").unwrap();
    assert_eq!(stuck.check.state, ServiceState::Down);
    assert_eq!(stuck.host_online, Some(OnlineState::AgentUnreachable));
    assert!(fleet.services.iter().any(|s| s.group == "Apps"));
    assert!(fleet.services.iter().any(|s| s.group == "Media"));
}
