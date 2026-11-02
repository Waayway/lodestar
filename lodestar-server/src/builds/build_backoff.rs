//! Backing off from builds that could not run (the builder was unreachable, a publish or the
//! gate failed), per channel and commit, so the prebuild and the re-send don't hammer a builder
//! that is down. Applies never wait for it.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;

use crate::clock;

/// The first and the longest pause after a build that could not run.
const FIRST: Duration = Duration::from_secs(30);
const LONGEST: Duration = Duration::from_secs(600);

/// The next attempt per (channel, commit): doubles from `first` up to `longest` after each
/// failure, gone after a success.
pub struct BuildBackoff {
    first: i64,
    longest: i64,
    /// (channel, commit) → (current pause, next attempt in unix seconds).
    waiting: Mutex<BTreeMap<(String, String), (i64, i64)>>,
}

impl Default for BuildBackoff {
    fn default() -> Self {
        Self::new(FIRST, LONGEST)
    }
}

impl BuildBackoff {
    pub fn new(first: Duration, longest: Duration) -> Self {
        Self {
            first: first.as_secs() as i64,
            longest: longest.as_secs() as i64,
            waiting: Mutex::default(),
        }
    }

    /// Records that building `commit` for `channel` could not run now.
    pub fn failed(&self, channel: &str, commit: &str) {
        self.failed_at(channel, commit, clock::now());
    }

    /// Records a failure at `now` (unix seconds).
    pub fn failed_at(&self, channel: &str, commit: &str, now: i64) {
        let mut waiting = self.lock();
        let key = (channel.to_string(), commit.to_string());
        let pause = match waiting.get(&key) {
            Some((pause, _)) => (pause * 2).clamp(self.first, self.longest),
            None => self.first,
        };
        waiting.insert(key, (pause, now + pause));
    }

    /// Ends the backoff of `commit` on `channel` (it built, or failed for good).
    pub fn succeeded(&self, channel: &str, commit: &str) {
        self.lock()
            .remove(&(channel.to_string(), commit.to_string()));
    }

    /// Whether a prebuild or re-send may try `commit` on `channel` now.
    pub fn ready(&self, channel: &str, commit: &str) -> bool {
        self.ready_at(channel, commit, clock::now())
    }

    /// Whether it may try at `now` (unix seconds).
    pub fn ready_at(&self, channel: &str, commit: &str, now: i64) -> bool {
        self.lock()
            .get(&(channel.to_string(), commit.to_string()))
            .is_none_or(|(_, next)| now >= *next)
    }

    /// Whether the last attempt of `commit` on `channel` could not run.
    pub fn waiting(&self, channel: &str, commit: &str) -> bool {
        self.lock()
            .contains_key(&(channel.to_string(), commit.to_string()))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<(String, String), (i64, i64)>> {
        self.waiting
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
