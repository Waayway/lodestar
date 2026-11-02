//! Marks a channel as building in `AppState.building` for as long as the guard lives.

use lodestar_proto::WsMessage;

use crate::state::{AppState, channels};

/// Sets `building[channel] = commit` and tells dashboards; dropping it (also on error or panic)
/// removes the entry, unless a newer build of the channel has replaced it meanwhile.
pub(crate) struct BuildingGuard {
    state: AppState,
    channel: String,
    commit: String,
}

impl BuildingGuard {
    pub(crate) fn start(state: &AppState, channel: &str, commit: &str) -> Self {
        state
            .building
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(channel.to_string(), commit.to_string());
        broadcast_channels(state);
        Self {
            state: state.clone(),
            channel: channel.to_string(),
            commit: commit.to_string(),
        }
    }
}

impl Drop for BuildingGuard {
    fn drop(&mut self) {
        let removed = {
            let mut building = self
                .state
                .building
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if building.get(&self.channel) == Some(&self.commit) {
                building.remove(&self.channel);
                true
            } else {
                false
            }
        };
        if removed {
            broadcast_channels(&self.state);
        }
    }
}

/// Sends `ChannelsUpdated` from a task (the channel list reads the mirror).
fn broadcast_channels(state: &AppState) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let state = state.clone();
    runtime.spawn(async move {
        let views = channels(&state).await;
        state.broadcast(WsMessage::ChannelsUpdated(views));
    });
}
