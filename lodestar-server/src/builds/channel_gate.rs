//! The build gate as the callers see it: one channel at one commit (apply, re-send, prebuild).

use super::building_guard::BuildingGuard;
use super::result_file::is_built_here;
use crate::config::ManagedHost;
use crate::state::AppState;

/// Every host on `channel` (the gate picks the Linux ones), when the gate applies to it: builds
/// are on and at least one of them is a Linux host. `None` means nothing waits on a build.
pub(crate) fn gated_hosts(state: &AppState, channel: &str) -> Option<Vec<ManagedHost>> {
    if !state.config.builds.enable {
        return None;
    }
    let hosts: Vec<ManagedHost> = state
        .hosts
        .iter()
        .filter(|host| host.channel == channel)
        .cloned()
        .collect();
    hosts.iter().any(is_built_here).then_some(hosts)
}

/// Whether an apply of `hosts` never waits on the gate: none of them is a Linux host (a
/// darwin-only apply). Any Linux host among them gates the whole channel.
pub fn skips_gate(hosts: &[&ManagedHost]) -> bool {
    !hosts.iter().any(|host| is_built_here(host))
}

/// What is known about `commit` on `channel` without building or waiting: `Some` with the
/// failed hosts (sorted; empty means ready, also when the gate does not apply) once every Linux
/// host of the channel has a result, `None` while one is still missing.
pub fn known_failures(state: &AppState, channel: &str, commit: &str) -> Option<Vec<String>> {
    let Some(hosts) = gated_hosts(state, channel) else {
        return Some(Vec::new());
    };
    let result = state.builds.read(commit)?;
    result
        .missing(&hosts)
        .is_empty()
        .then(|| sorted(result.failed_hosts(&hosts)))
}

/// Builds `commit` for `channel` through the gate (waiting for it) and returns the failed hosts,
/// sorted; empty when all are built or the gate does not apply. The channel shows as building
/// meanwhile. The work runs in a task of its own, so a caller that goes away does not end the
/// building state while the build goes on.
pub async fn build_channel(
    state: &AppState,
    channel: &str,
    commit: &str,
) -> anyhow::Result<Vec<String>> {
    let Some(hosts) = gated_hosts(state, channel) else {
        return Ok(Vec::new());
    };
    if let Some(failed) = known_failures(state, channel, commit) {
        if failed.is_empty() {
            // Built already: the gate still advances the channel to it (and publishes) when it
            // is the head and the index lags behind; that needs no building state.
            state.builds.ensure_built(state, commit, &hosts).await?;
        }
        return Ok(failed);
    }
    let task_state = state.clone();
    let (channel, commit) = (channel.to_string(), commit.to_string());
    let task = tokio::spawn(async move {
        let _building = BuildingGuard::start(&task_state, &channel, &commit);
        let started = std::time::Instant::now();
        let result = task_state
            .builds
            .ensure_built(&task_state, &commit, &hosts)
            .await?;
        task_state
            .build_durations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(channel.clone(), started.elapsed().as_secs_f64());
        Ok(sorted(result.failed_hosts(&hosts)))
    });
    match task.await {
        Ok(result) => result,
        Err(error) if error.is_panic() => anyhow::bail!("the build task panicked"),
        Err(error) => anyhow::bail!("the build task was cancelled: {error}"),
    }
}

fn sorted(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    names
}
