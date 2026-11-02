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

/// Queues a prebuild of `head` for `channel`. The returned task waits its turn, then builds the
/// newest head queued for the channel by then, or does nothing when an earlier task took it.
pub fn spawn_prebuild(state: &AppState, channel: &str, head: &str) -> JoinHandle<()> {
    state
        .prebuilds
        .pending
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(channel.to_string(), head.to_string());
    let state = state.clone();
    let channel = channel.to_string();
    tokio::spawn(async move {
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
    })
}

/// After a mirror fetch: queues a prebuild for every gated channel whose head differs from the one
/// it had at the previous call (the first call, at server start, prebuilds every head once).
/// Returns the queued tasks; none while builds are off.
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
        if moved {
            tasks.push(spawn_prebuild(state, channel, &head));
        }
    }
    tasks
}
