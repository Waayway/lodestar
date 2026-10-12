use lodestar_proto::{AgentStatus, ApplyRecord, ApplyState, CheckoutStatus, SkipReason};
use lodestar_server::db;
use serde_json::json;

const C1: &str = "0123456789abcdef0123456789abcdef01234567";

fn record(id: &str, state: ApplyState) -> ApplyRecord {
    ApplyRecord {
        apply_id: id.into(),
        commit: C1.into(),
        state,
        skip_reason: None,
        message: None,
        exit_code: None,
        started_at: None,
        finished_at: None,
    }
}

#[tokio::test]
async fn migrations_run_twice_ok() {
    let dir = tempfile::tempdir().unwrap();
    db::connect(dir.path()).await.unwrap();
    let connection = db::connect(dir.path()).await.unwrap();
    assert_eq!(db::activations_next(&connection, "h").await.unwrap(), 0);
}

#[tokio::test]
async fn desired_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let connection = db::connect(dir.path()).await.unwrap();
    assert!(db::desired(&connection, "h1").await.unwrap().is_none());
    db::insert_apply(
        &connection,
        "h1",
        "alice",
        100,
        &record("a1", ApplyState::Idle),
    )
    .await
    .unwrap();
    db::set_desired(&connection, "h1", "a1").await.unwrap();
    let desired = db::desired(&connection, "h1").await.unwrap().unwrap();
    assert_eq!(
        (
            desired.apply_id.as_str(),
            desired.commit.as_str(),
            desired.requested_by.as_str()
        ),
        ("a1", C1, "alice")
    );
    assert_eq!(desired.state, "idle");

    let mut updated = record("a1", ApplyState::Skipped);
    updated.skip_reason = Some(SkipReason::Dirty);
    updated.message = Some("dirty".into());
    updated.started_at = Some(101);
    updated.finished_at = Some(102);
    db::update_apply_from_record(&connection, &updated)
        .await
        .unwrap();
    db::insert_apply(
        &connection,
        "h1",
        "bob",
        200,
        &record("a2", ApplyState::Idle),
    )
    .await
    .unwrap();
    db::set_desired(&connection, "h1", "a2").await.unwrap();
    assert_eq!(
        db::desired(&connection, "h1")
            .await
            .unwrap()
            .unwrap()
            .apply_id,
        "a2"
    );
    db::set_desired(&connection, "h1", "a1").await.unwrap();
    let desired = db::desired(&connection, "h1").await.unwrap().unwrap();
    assert_eq!(desired.state, "skipped");
    assert_eq!(desired.skip_reason.as_deref(), Some("dirty"));
    assert_eq!(desired.finished_at, Some(102));
    assert_eq!(desired.requested_at, 100);
}

#[tokio::test]
async fn audit_insert_and_list_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let connection = db::connect(dir.path()).await.unwrap();
    db::insert_audit(
        &connection,
        10,
        Some("h1"),
        "apply",
        "alice",
        json!({"n": 1}),
    )
    .await
    .unwrap();
    db::insert_audit(&connection, 30, None, "promote", "bob", json!({"n": 2}))
        .await
        .unwrap();
    db::insert_audit(
        &connection,
        20,
        Some("h2"),
        "apply",
        "carol",
        json!({"n": 3}),
    )
    .await
    .unwrap();
    let events = db::recent_audit(&connection, 2).await.unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(
        (
            events[0].time,
            events[0].host.clone(),
            events[0].details.clone()
        ),
        (30, None, json!({"n": 2}))
    );
    assert_eq!(events[1].time, 20);
}

#[tokio::test]
async fn status_roundtrip_json() {
    let dir = tempfile::tempdir().unwrap();
    let connection = db::connect(dir.path()).await.unwrap();
    assert!(db::load_status(&connection, "h1").await.unwrap().is_none());
    let agent_status = AgentStatus {
        host: "h1".into(),
        os: "nixos".into(),
        agent_version: "0.1.0".into(),
        running: C1.into(),
        checkout: CheckoutStatus {
            branch: Some("main".into()),
            head: Some(C1.into()),
            dirty: false,
            untracked: 0,
            ahead: Some(0),
            last_fetch_at: Some(5),
            flake_path: None,
        },
        apply: Some(record("a1", ApplyState::Ok)),
    };
    db::save_status(&connection, "h1", &agent_status, 1234)
        .await
        .unwrap();
    let (got, ok_at) = db::load_status(&connection, "h1").await.unwrap().unwrap();
    assert_eq!(got, agent_status);
    assert_eq!(ok_at, Some(1234));
    db::set_activations_next(&connection, "h1", 7)
        .await
        .unwrap();
    assert_eq!(db::activations_next(&connection, "h1").await.unwrap(), 7);
    // saving status must not reset the cursor
    db::save_status(&connection, "h1", &agent_status, 1300)
        .await
        .unwrap();
    assert_eq!(db::activations_next(&connection, "h1").await.unwrap(), 7);
}
