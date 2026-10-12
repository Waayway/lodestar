mod common;
use common::*;
use lodestar_agent::checks::check_apply;
use lodestar_agent::git::Git;
use lodestar_proto::SkipReason;

#[test]
fn ok_when_clean_and_ff() {
    let fixture = fixture();
    let commit = push_remote_commit(&fixture, "b.txt");
    let git_runner = Git::new(&fixture.config).unwrap();
    git_runner.fetch(&fixture.config.repo_url).unwrap();
    assert_eq!(check_apply(&git_runner, "unstable", &commit), Ok(()));
}

#[test]
fn missing_commit() {
    let fixture = fixture();
    let git_runner = Git::new(&fixture.config).unwrap();
    assert_eq!(
        check_apply(&git_runner, "unstable", &"0".repeat(40)),
        Err(SkipReason::CommitNotFound)
    );
}

#[test]
fn commit_only_on_other_branch() {
    let fixture = fixture();
    git(&fixture.clone, &["checkout", "-q", "-b", "other"]);
    let commit = commit_file(&fixture.clone, "o.txt", "o");
    git(&fixture.clone, &["checkout", "-q", "unstable"]);
    let git_runner = Git::new(&fixture.config).unwrap();
    assert_eq!(
        check_apply(&git_runner, "unstable", &commit),
        Err(SkipReason::CommitNotOnChannel)
    );
}

#[test]
fn no_remote_channel_ref() {
    let fixture = fixture();
    git(
        &fixture.clone,
        &["update-ref", "-d", "refs/remotes/origin/stable"],
    );
    let head = git(&fixture.clone, &["rev-parse", "HEAD"]);
    let git_runner = Git::new(&fixture.config).unwrap();
    assert_eq!(
        check_apply(&git_runner, "stable", &head),
        Err(SkipReason::CommitNotOnChannel)
    );
}

#[test]
fn detached_head() {
    let fixture = fixture();
    let commit = push_remote_commit(&fixture, "b.txt");
    git(&fixture.clone, &["checkout", "-q", "--detach"]);
    let git_runner = Git::new(&fixture.config).unwrap();
    git_runner.fetch(&fixture.config.repo_url).unwrap();
    assert_eq!(
        check_apply(&git_runner, "unstable", &commit),
        Err(SkipReason::WrongBranch)
    );
}

#[test]
fn tracked_change() {
    let fixture = fixture();
    let commit = push_remote_commit(&fixture, "b.txt");
    let git_runner = Git::new(&fixture.config).unwrap();
    git_runner.fetch(&fixture.config.repo_url).unwrap();
    std::fs::write(fixture.clone.join("a.txt"), "changed").unwrap();
    assert_eq!(
        check_apply(&git_runner, "unstable", &commit),
        Err(SkipReason::Dirty)
    );
}

#[test]
fn untracked_only_is_ok() {
    let fixture = fixture();
    let commit = push_remote_commit(&fixture, "b.txt");
    let git_runner = Git::new(&fixture.config).unwrap();
    git_runner.fetch(&fixture.config.repo_url).unwrap();
    std::fs::write(fixture.clone.join("scratch.txt"), "x").unwrap();
    assert_eq!(check_apply(&git_runner, "unstable", &commit), Ok(()));
}

#[test]
fn local_commit_diverged() {
    let fixture = fixture();
    let commit = push_remote_commit(&fixture, "b.txt");
    commit_file(&fixture.clone, "local.txt", "l");
    let git_runner = Git::new(&fixture.config).unwrap();
    git_runner.fetch(&fixture.config.repo_url).unwrap();
    assert_eq!(
        check_apply(&git_runner, "unstable", &commit),
        Err(SkipReason::NotFastForward)
    );
}
