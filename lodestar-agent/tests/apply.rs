mod common;
use common::*;
use lodestar_agent::apply::{
    applies_dir, clear_stale_marker, latest_record, prune, read_log, read_record, run_apply,
    settle_running, try_lock_apply,
};
use lodestar_agent::git::Git;
use lodestar_proto::{ApplyRecord, ApplyState, SkipReason};

fn with_command(fixture: &mut Fixture, script: &str) {
    fixture.config.rebuild_command = Some(vec!["sh".into(), "-c".into(), script.into()]);
}

fn fetch_new_commit(fixture: &Fixture) -> String {
    let commit = push_remote_commit(fixture, "b.txt");
    Git::new(&fixture.config)
        .unwrap()
        .fetch(&fixture.config.repo_url)
        .unwrap();
    commit
}

fn write_record(fixture: &Fixture, id: &str, state: ApplyState, started_at: i64) {
    let dir = applies_dir(&fixture.config);
    std::fs::create_dir_all(&dir).unwrap();
    let record = ApplyRecord {
        apply_id: id.into(),
        commit: "0".repeat(40),
        state,
        skip_reason: None,
        message: None,
        exit_code: None,
        started_at: Some(started_at),
        finished_at: None,
    };
    std::fs::write(
        dir.join(format!("{id}.json")),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
}

#[test]
fn apply_ok_moves_head_and_records_ok() {
    let mut fixture = fixture();
    with_command(&mut fixture, "echo built $0 $1; exit 0");
    let commit = fetch_new_commit(&fixture);
    let record = run_apply(&fixture.config, "id-1", &commit);
    assert_eq!(record.state, ApplyState::Ok, "{record:?}");
    assert_eq!(record.exit_code, Some(0));
    assert!(record.started_at.is_some() && record.finished_at.is_some());
    assert_eq!(git(&fixture.clone, &["rev-parse", "HEAD"]), commit);
    let log = read_log(&fixture.config, "id-1", 0).unwrap();
    assert!(
        log.data.contains(&format!("built h {commit}")),
        "{}",
        log.data
    );
    assert!(
        !fixture
            .config
            .state_dir
            .join("apply-in-progress.json")
            .exists()
    );
    assert_eq!(read_record(&fixture.config, "id-1"), Some(record));
}

#[test]
fn apply_skips_dirty_and_leaves_tree() {
    let mut fixture = fixture();
    with_command(&mut fixture, "echo built; exit 0");
    let commit = fetch_new_commit(&fixture);
    let before = git(&fixture.clone, &["rev-parse", "HEAD"]);
    std::fs::write(fixture.clone.join("a.txt"), "changed").unwrap();
    let record = run_apply(&fixture.config, "id-2", &commit);
    assert_eq!(record.state, ApplyState::Skipped);
    assert_eq!(record.skip_reason, Some(SkipReason::Dirty));
    assert_eq!(git(&fixture.clone, &["rev-parse", "HEAD"]), before);
    assert!(
        !fixture
            .config
            .state_dir
            .join("apply-in-progress.json")
            .exists()
    );
}

#[test]
fn apply_failed_build_records_exit_code() {
    let mut fixture = fixture();
    with_command(&mut fixture, "echo nope >&2; exit 3");
    let commit = fetch_new_commit(&fixture);
    let record = run_apply(&fixture.config, "id-3", &commit);
    assert_eq!(record.state, ApplyState::Failed);
    assert_eq!(record.exit_code, Some(3));
    assert_eq!(git(&fixture.clone, &["rev-parse", "HEAD"]), commit);
    assert!(
        read_log(&fixture.config, "id-3", 0)
            .unwrap()
            .data
            .contains("nope")
    );
}

#[test]
fn apply_rejects_invalid_commit() {
    let mut fixture = fixture();
    with_command(&mut fixture, "exit 0");
    let record = run_apply(&fixture.config, "id-4", "--upload-pack=evil");
    assert_eq!(record.state, ApplyState::Failed);
    assert_eq!(record.message.as_deref(), Some("invalid commit"));
}

#[test]
fn apply_fetch_failure_is_skipped() {
    let mut fixture = fixture();
    with_command(&mut fixture, "exit 0");
    fixture.config.repo_url = fixture
        .config
        .state_dir
        .join("missing.git")
        .to_string_lossy()
        .into();
    let record = run_apply(&fixture.config, "id-5", &"a".repeat(40));
    assert_eq!(record.state, ApplyState::Skipped);
    assert_eq!(record.skip_reason, Some(SkipReason::FetchFailed));
}

#[test]
fn interrupted_running_record_reports_failed() {
    let fixture = fixture();
    write_record(&fixture, "dead", ApplyState::Running, 100);
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    std::fs::write(
        applies_dir(&fixture.config).join("dead.pid"),
        pid.to_string(),
    )
    .unwrap();
    let record = latest_record(&fixture.config).unwrap();
    assert_eq!(record.apply_id, "dead");
    assert_eq!(record.state, ApplyState::Failed);
    assert_eq!(record.message.as_deref(), Some("interrupted"));
}

#[test]
fn running_record_with_live_pid_stays_running() {
    let fixture = fixture();
    // A running wrapper holds the apply lock.
    let _lock = try_lock_apply(&fixture.config).unwrap();
    write_record(&fixture, "live", ApplyState::Running, 100);
    std::fs::write(
        applies_dir(&fixture.config).join("live.pid"),
        std::process::id().to_string(),
    )
    .unwrap();
    assert_eq!(
        latest_record(&fixture.config).unwrap().state,
        ApplyState::Running
    );
}

#[test]
fn running_record_with_reused_pid_and_no_lock_is_interrupted() {
    let fixture = fixture();
    let marker = fixture.config.state_dir.join("apply-in-progress.json");
    std::fs::create_dir_all(&fixture.config.state_dir).unwrap();
    std::fs::write(&marker, "{}").unwrap();
    write_record(&fixture, "reused", ApplyState::Running, 100);
    // A live but unrelated process (this test) has the recorded pid; no wrapper holds the lock.
    let pid_file = applies_dir(&fixture.config).join("reused.pid");
    std::fs::write(&pid_file, std::process::id().to_string()).unwrap();
    let record = latest_record(&fixture.config).unwrap();
    assert_eq!(record.state, ApplyState::Failed);
    assert_eq!(record.message.as_deref(), Some("interrupted"));
    assert!(record.finished_at.is_some());
    // Made permanent: the stored record says so, the pid file and the marker are gone.
    let raw = std::fs::read(applies_dir(&fixture.config).join("reused.json")).unwrap();
    let stored: ApplyRecord = serde_json::from_slice(&raw).unwrap();
    assert_eq!(stored.state, ApplyState::Failed);
    assert!(!pid_file.exists());
    assert!(!marker.exists());
    // And it can be pruned like any finished apply.
    for index in 0..3 {
        write_record(&fixture, &format!("n{index}"), ApplyState::Ok, 200 + index);
    }
    prune(&fixture.config, 3);
    assert!(read_record(&fixture.config, "reused").is_none());
}

#[test]
fn dead_pid_while_another_wrapper_holds_the_lock_keeps_the_marker() {
    let fixture = fixture();
    let marker = fixture.config.state_dir.join("apply-in-progress.json");
    std::fs::create_dir_all(&fixture.config.state_dir).unwrap();
    std::fs::write(&marker, "{}").unwrap();
    let _lock = try_lock_apply(&fixture.config).unwrap();
    write_record(&fixture, "dead", ApplyState::Running, 100);
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    std::fs::write(
        applies_dir(&fixture.config).join("dead.pid"),
        pid.to_string(),
    )
    .unwrap();
    let record = read_record(&fixture.config, "dead").unwrap();
    assert_eq!(record.state, ApplyState::Failed);
    // The marker may belong to the wrapper holding the lock.
    assert!(marker.exists());
}

#[test]
fn stale_marker_cleared_only_when_unlocked() {
    let fixture = fixture();
    let marker = fixture.config.state_dir.join("apply-in-progress.json");
    std::fs::create_dir_all(&fixture.config.state_dir).unwrap();
    std::fs::write(&marker, "{}").unwrap();
    let lock = try_lock_apply(&fixture.config).unwrap();
    clear_stale_marker(&fixture.config);
    assert!(marker.exists(), "a wrapper holds the lock");
    drop(lock);
    clear_stale_marker(&fixture.config);
    assert!(!marker.exists());
    clear_stale_marker(&fixture.config); // no marker: nothing to do
}

#[test]
fn latest_record_is_newest_by_started_at() {
    let fixture = fixture();
    write_record(&fixture, "old", ApplyState::Ok, 10);
    write_record(&fixture, "new", ApplyState::Ok, 20);
    assert_eq!(latest_record(&fixture.config).unwrap().apply_id, "new");
    assert_eq!(latest_record(&common::fixture().config), None);
}

#[test]
fn prune_keeps_50() {
    let fixture = fixture();
    for index in 0..60 {
        let id = format!("a{index:02}");
        write_record(&fixture, &id, ApplyState::Ok, index);
        std::fs::write(applies_dir(&fixture.config).join(format!("{id}.log")), "x").unwrap();
    }
    prune(&fixture.config, 50);
    let json = |name: &str| {
        applies_dir(&fixture.config)
            .join(format!("{name}.json"))
            .exists()
    };
    let log = |name: &str| {
        applies_dir(&fixture.config)
            .join(format!("{name}.log"))
            .exists()
    };
    assert!(!json("a00") && !log("a09") && !json("a09"));
    assert!(json("a10") && log("a10") && json("a59"));
    let count = std::fs::read_dir(applies_dir(&fixture.config))
        .unwrap()
        .filter(|entry| {
            entry
                .as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|extension| extension == "json")
        })
        .count();
    assert_eq!(count, 50);
}

#[test]
fn read_log_offset() {
    let fixture = fixture();
    write_record(&fixture, "r", ApplyState::Ok, 1);
    std::fs::write(applies_dir(&fixture.config).join("r.log"), "hello world").unwrap();
    let chunk = read_log(&fixture.config, "r", 6).unwrap();
    assert_eq!(
        (chunk.data.as_str(), chunk.offset, chunk.next, chunk.eof),
        ("world", 6, 11, true)
    );
    let chunk = read_log(&fixture.config, "r", 100).unwrap();
    assert_eq!(
        (chunk.data.as_str(), chunk.next, chunk.eof),
        ("", 100, true)
    );
    assert!(read_log(&fixture.config, "missing", 0).is_none());
    assert!(read_log(&fixture.config, "../r", 0).is_none());
}

#[test]
fn read_log_not_eof_while_running() {
    let fixture = fixture();
    let _lock = try_lock_apply(&fixture.config).unwrap();
    write_record(&fixture, "run", ApplyState::Running, 1);
    std::fs::write(
        applies_dir(&fixture.config).join("run.pid"),
        std::process::id().to_string(),
    )
    .unwrap();
    std::fs::write(applies_dir(&fixture.config).join("run.log"), "partial").unwrap();
    let chunk = read_log(&fixture.config, "run", 0).unwrap();
    assert_eq!((chunk.data.as_str(), chunk.eof), ("partial", false));
}

#[test]
fn read_log_does_not_split_multibyte_char_while_running() {
    let fixture = fixture();
    let _lock = try_lock_apply(&fixture.config).unwrap();
    write_record(&fixture, "mb", ApplyState::Running, 1);
    let dir = applies_dir(&fixture.config);
    std::fs::write(dir.join("mb.pid"), std::process::id().to_string()).unwrap();
    // "a" then the first byte of "é" (c3 a9).
    std::fs::write(dir.join("mb.log"), [b'a', 0xc3]).unwrap();
    let chunk = read_log(&fixture.config, "mb", 0).unwrap();
    assert_eq!(
        (chunk.data.as_str(), chunk.next, chunk.eof),
        ("a", 1, false)
    );
    std::fs::write(dir.join("mb.log"), [b'a', 0xc3, 0xa9]).unwrap();
    let chunk = read_log(&fixture.config, "mb", chunk.next).unwrap();
    assert_eq!((chunk.data.as_str(), chunk.next), ("é", 3));
}

#[test]
fn read_log_next_counts_raw_bytes_past_invalid_utf8() {
    let fixture = fixture();
    write_record(&fixture, "bad", ApplyState::Ok, 1);
    // An invalid byte mid-chunk becomes U+FFFD (3 bytes of text for 1 byte of log).
    std::fs::write(
        applies_dir(&fixture.config).join("bad.log"),
        b"a\xffb\nend\n",
    )
    .unwrap();
    let chunk = read_log(&fixture.config, "bad", 0).unwrap();
    assert_eq!(
        (chunk.data.as_str(), chunk.next, chunk.eof),
        ("a\u{fffd}b\nend\n", 8, true)
    );
    assert_ne!(chunk.data.len() as u64, chunk.next);
    let chunk = read_log(&fixture.config, "bad", 4).unwrap();
    assert_eq!(
        (chunk.data.as_str(), chunk.offset, chunk.next),
        ("end\n", 4, 8)
    );
}

#[test]
fn launch_command_rejects_bad_input() {
    use lodestar_agent::launcher::launch_command;
    let fixture = fixture();
    let config_path = std::path::Path::new("/c.json");
    let ok = "a".repeat(40);
    assert!(launch_command(&fixture.config, config_path, "-x", &ok).is_err());
    assert!(launch_command(&fixture.config, config_path, "ééééé", &ok).is_err());
    assert!(launch_command(&fixture.config, config_path, "id", "--evil").is_err());
}

#[test]
fn launch_command_systemd_run_argv() {
    use lodestar_agent::config::Launcher;
    use lodestar_agent::launcher::{DAEMON_PATH, launch_command};
    let mut fixture = fixture();
    fixture.config.launcher = Launcher::SystemdRun;
    fixture.config.agent_exe = Some("/bin/fa".into());
    let commit = "b".repeat(40);
    let (program, args) = launch_command(
        &fixture.config,
        std::path::Path::new("/c.json"),
        "12345678-90",
        &commit,
    )
    .unwrap();
    assert_eq!(program.to_str(), Some("systemd-run"));
    let want = [
        "--unit",
        "lodestar-apply-12345678",
        "--collect",
        "--property=Type=exec",
        &format!("--setenv=PATH={DAEMON_PATH}"),
        "--setenv=HOME=/root",
        "/bin/fa",
        "apply",
        "--config",
        "/c.json",
        "--",
        "12345678-90",
        &commit,
    ];
    assert_eq!(args, want);
}

#[test]
fn launch_command_detach_uses_agent_exe() {
    use lodestar_agent::config::Launcher;
    use lodestar_agent::launcher::launch_command;
    let mut fixture = fixture();
    fixture.config.launcher = Launcher::Detach;
    fixture.config.agent_exe = Some("/bin/fa".into());
    let commit = "b".repeat(40);
    let (program, args) = launch_command(
        &fixture.config,
        std::path::Path::new("/c.json"),
        "id1",
        &commit,
    )
    .unwrap();
    assert_eq!(program.to_str(), Some("/bin/fa"));
    assert_eq!(args, ["apply", "--config", "/c.json", "--", "id1", &commit]);
}

#[test]
fn daemon_home_is_roots_home_per_os() {
    use lodestar_agent::launcher::daemon_home;
    let mut fixture = fixture();
    fixture.config.os = "darwin".into();
    assert_eq!(daemon_home(&fixture.config), "/var/root");
    fixture.config.os = "linux".into();
    assert_eq!(daemon_home(&fixture.config), "/root");
}

/// The wrapper finishes between the daemon's read of `<id>.json` (still `Running`) and its
/// lock check: the final record must survive, not be overwritten as "interrupted".
#[test]
fn wrapper_finishing_after_the_read_keeps_its_result() {
    let fixture = fixture();
    // The wrapper was running: Running record, its pid file and the in-progress marker.
    write_record(&fixture, "race", ApplyState::Running, 100);
    std::fs::write(
        applies_dir(&fixture.config).join("race.pid"),
        std::process::id().to_string(),
    )
    .unwrap();
    // The daemon reads the record while the wrapper still runs.
    let stale: ApplyRecord = serde_json::from_slice(
        &std::fs::read(applies_dir(&fixture.config).join("race.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(stale.state, ApplyState::Running);
    // Now the wrapper finishes: final record, pid file removed, lock released (never held here).
    write_record(&fixture, "race", ApplyState::Ok, 100);
    std::fs::remove_file(applies_dir(&fixture.config).join("race.pid")).unwrap();
    // The daemon continues with its stale copy: pid gone and lock free.
    let record = settle_running(&fixture.config, stale);
    assert_eq!(record.state, ApplyState::Ok, "{record:?}");
    assert_eq!(record.message, None);
    // And the file on disk still says Ok.
    let on_disk = read_record(&fixture.config, "race").unwrap();
    assert_eq!(on_disk.state, ApplyState::Ok, "{on_disk:?}");
}
