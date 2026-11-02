//! Prebuilds: after a mirror fetch, every channel whose head moved is built in the background, so
//! an apply of it finds the build done.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use tokio::task::JoinHandle;

use super::channel_gate::{build_channel, gated_hosts};
use crate::state::AppState;

/// One prebuild at a time; prebuilds still waiting for their turn collapse to the newest head of
/// their channel.
#[derive(Default)]
pub struct PrebuildQueue {
    /// Held by the prebuild that is running.
    run: tokio::sync::Mutex<()>,
    /// The newest head waiting to be prebuilt, per channel.
    pending: Mutex<BTreeMap<String, String>>,
    /// The head each channel had at the last fetch (none yet at server start).
    seen: Mutex<BTreeMap<String, String>>,
}

/// Queues a prebuild of `head` for `channel`. A new task is spawned only when the channel had
/// nothing queued (at most one waiting task per channel); otherwise the waiting task builds this
/// newer head instead, and `None` is returned. The task waits its turn, then builds the newest
/// head queued for the channel by then.
pub fn spawn_prebuild(state: &AppState, channel: &str, head: &str) -> Option<JoinHandle<()>> {
    let queued = state
        .prebuilds
        .pending
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(channel.to_string(), head.to_string());
    if queued.is_some() {
        return None;
    }
    let state = state.clone();
    let channel = channel.to_string();
    Some(tokio::spawn(async move {
        let _run = state.prebuilds.run.lock().await;
        let head = state
            .prebuilds
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&channel);
        let Some(head) = head else {
            return;
        };
        match build_channel(&state, &channel, &head).await {
            Ok(failed) if failed.is_empty() => {}
            Ok(failed) => eprintln!(
                "lodestar-server: prebuild of {channel} at {head} failed on the builder: {}",
                failed.join(", ")
            ),
            Err(error) => eprintln!("lodestar-server: prebuild of {channel} at {head}: {error:#}"),
        }
    }))
}

/// After a mirror fetch: queues a prebuild for every gated channel whose head differs from the one
/// it had at the previous call (the first call, at server start, prebuilds every head once), and
/// for every head whose last build could not run once its backoff has passed (30 s doubling to
/// 10 min, checked at each fetch). Returns the tasks it spawned; none while builds are off.
pub async fn prebuild_new_heads(state: &AppState) -> Vec<JoinHandle<()>> {
    if !state.config.builds.enable {
        return Vec::new();
    }
    let channels: BTreeSet<&str> = state
        .hosts
        .iter()
        .map(|host| host.channel.as_str())
        .filter(|channel| gated_hosts(state, channel).is_some())
        .collect();
    let mut tasks = Vec::new();
    for channel in channels {
        let Some(head) = state.mirror.head(channel).await else {
            continue;
        };
        let moved = state
            .prebuilds
            .seen
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(channel.to_string(), head.clone())
            .as_ref()
            != Some(&head);
        // A head whose build could not run is tried again once its backoff has passed.
        let due = state.build_backoff.waiting(channel, &head)
            && state.build_backoff.ready(channel, &head);
        if moved || due {
            tasks.extend(spawn_prebuild(state, channel, &head));
        }
    }
    tasks
}
