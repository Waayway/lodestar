//! Bare git mirror of the config repo: channel heads, behind counts, commit
//! subjects and the promote push (by default `unstable` -> `stable`).
//!
//! Every git call goes through `tokio::process::Command` with the host's git
//! configuration disabled and prompts off. Commit hashes are validated with
//! `is_commit_hash` and channel names are checked before they reach git, and
//! refs are always passed fully qualified, so a caller string can never be
//! parsed as an option.

mod git_command;
#[allow(clippy::module_inception)]
mod mirror;
mod promote_error;

pub use mirror::Mirror;
pub use promote_error::PromoteError;
