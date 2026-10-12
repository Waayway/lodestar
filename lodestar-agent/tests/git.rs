mod common;
use common::*;
use lodestar_agent::git::Git;

#[test]
fn fetch_then_commit_exists() {
    let fixture = fixture();
    let commit = push_remote_commit(&fixture, "b.txt");
    let git_runner = Git::new(&fixture.config).unwrap();
    assert!(!git_runner.commit_exists(&commit));
    git_runner.fetch(&fixture.config.repo_url).unwrap();
    assert!(git_runner.commit_exists(&commit));
    assert!(!git_runner.commit_exists(&"0".repeat(40)));
    assert!(git_runner.fetch("/nonexistent/repo").is_err());
}

#[test]
fn fetch_within_works_and_times_out() {
    use std::time::{Duration, Instant};
    let fixture = fixture();
    let commit = push_remote_commit(&fixture, "b.txt");
    let git_runner = Git::new(&fixture.config).unwrap();
    git_runner
        .fetch_within(&fixture.config.repo_url, Duration::from_secs(30))
        .unwrap();
    assert!(git_runner.commit_exists(&commit));
    assert!(
        git_runner
            .fetch_within("/nonexistent/repo", Duration::from_secs(30))
            .is_err()
    );

    // A git:// server that accepts and never answers.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("git://{}/repo", listener.local_addr().unwrap());
    let hold = std::thread::spawn(move || {
        let connection = listener.accept();
        std::thread::sleep(Duration::from_secs(5));
        drop(connection);
    });
    let start = Instant::now();
    let error = git_runner
        .fetch_within(&url, Duration::from_secs(1))
        .unwrap_err();
    assert!(format!("{error:#}").contains("timed out"), "{error:#}");
    assert!(
        start.elapsed() < Duration::from_secs(4),
        "{:?}",
        start.elapsed()
    );
    hold.join().unwrap();
}

#[test]
fn dirty_tracked_only() {
    let fixture = fixture();
    let git_runner = Git::new(&fixture.config).unwrap();
    assert!(!git_runner.is_dirty());
    std::fs::write(fixture.clone.join("new.txt"), "x").unwrap();
    assert!(!git_runner.is_dirty());
    assert_eq!(git_runner.untracked_count(), 1);
    std::fs::write(fixture.clone.join("a.txt"), "changed").unwrap();
    assert!(git_runner.is_dirty());
}

#[test]
fn detached_head_branch_is_none() {
    let fixture = fixture();
    let git_runner = Git::new(&fixture.config).unwrap();
    assert_eq!(git_runner.current_branch().as_deref(), Some("unstable"));
    let head = git_runner.head().unwrap();
    git(&fixture.clone, &["checkout", "-q", "--detach", &head]);
    assert_eq!(git_runner.current_branch(), None);
    assert_eq!(git_runner.head().as_deref(), Some(head.as_str()));
}

#[test]
fn ahead_counts_local_commits() {
    let fixture = fixture();
    let git_runner = Git::new(&fixture.config).unwrap();
    assert_eq!(git_runner.ahead_of("origin/unstable"), Some(0));
    commit_file(&fixture.clone, "l1.txt", "1");
    commit_file(&fixture.clone, "l2.txt", "2");
    assert_eq!(git_runner.ahead_of("origin/unstable"), Some(2));
    assert_eq!(git_runner.ahead_of("origin/nope"), None);
}

#[test]
fn ff_merge_moves_head() {
    let fixture = fixture();
    let before = Git::new(&fixture.config).unwrap().head().unwrap();
    let commit = push_remote_commit(&fixture, "b.txt");
    let git_runner = Git::new(&fixture.config).unwrap();
    git_runner.fetch(&fixture.config.repo_url).unwrap();
    assert!(git_runner.is_ancestor(&before, &commit));
    assert!(!git_runner.is_ancestor(&commit, &before));
    git_runner.ff_merge(&commit).unwrap();
    assert_eq!(git_runner.head().unwrap(), commit);
    // diverged history cannot fast-forward
    git(&fixture.clone, &["reset", "-q", "--hard", &before]);
    commit_file(&fixture.clone, "local.txt", "l");
    assert!(git_runner.ff_merge(&commit).is_err());
}
