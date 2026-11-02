//! What the dashboard, `GET /api/builds` and the metrics know of a channel's build.

use std::collections::BTreeMap;

use lodestar_proto::ChannelBuildView;

use super::build_metric::BuildMetric;
use super::channel_gate::gated_hosts;
use crate::state::AppState;

/// The recorded build of `commit` on `channel`: `failed` when any Linux host of the channel
/// failed, `ok` when every one is ok, `None` while one has no result (or builds are off).
fn recorded(state: &AppState, channel: &str, commit: &str) -> Option<ChannelBuildView> {
    let hosts = gated_hosts(state, channel)?;
    let result = state.builds.read(commit)?;
    let mut failed = result.failed_hosts(&hosts);
    failed.sort();
    if failed.is_empty() && !result.missing(&hosts).is_empty() {
        return None;
    }
    let finished_at = hosts
        .iter()
        .filter_map(|host| result.hosts.get(&host.name))
        .map(|host_result| host_result.built_at)
        .max();
    Some(ChannelBuildView {
        commit: commit.to_string(),
        state: if failed.is_empty() { "ok" } else { "failed" }.to_string(),
        failed_hosts: failed,
        finished_at,
    })
}

/// The build state of `channel` whose head is `head`: `building` while the gate builds for it,
/// else the recorded result of the head. `None` when builds are off, the channel has no Linux
/// host or the head has no (complete) result yet.
pub fn channel_build(
    state: &AppState,
    channel: &str,
    head: Option<&str>,
) -> Option<ChannelBuildView> {
    gated_hosts(state, channel)?;
    let building = state
        .building
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(channel)
        .cloned();
    if let Some(commit) = building {
        return Some(ChannelBuildView {
            commit,
            state: "building".to_string(),
            failed_hosts: Vec::new(),
            finished_at: None,
        });
    }
    recorded(state, channel, head?)
}

/// The metrics per gated channel that has a recorded build of its head.
pub async fn build_metrics(state: &AppState) -> BTreeMap<String, BuildMetric> {
    let channels: std::collections::BTreeSet<&str> = state
        .hosts
        .iter()
        .map(|host| host.channel.as_str())
        .collect();
    let durations = state
        .build_durations
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    let mut metrics = BTreeMap::new();
    for channel in channels {
        let Some(head) = state.mirror.head(channel).await else {
            continue;
        };
        let Some(build) = recorded(state, channel, &head) else {
            continue;
        };
        metrics.insert(
            channel.to_string(),
            BuildMetric {
                result: build.state,
                finished_at: build.finished_at.unwrap_or(0),
                duration_seconds: durations.get(channel).copied(),
            },
        );
    }
    metrics
}
