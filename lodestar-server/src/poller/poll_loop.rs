use std::time::Duration;

use lodestar_proto::WsMessage;
use tokio::time::MissedTickBehavior;

use super::host_poll::poll_once;
use crate::state::{AppState, channels};

/// Background loops: mirror fetch every `mirror_fetch_secs` (then `ChannelsUpdated`) and
/// agent polling every `poll_secs`. Both run once immediately. Returns both tasks.
pub fn spawn(state: AppState) -> Vec<tokio::task::JoinHandle<()>> {
    let mirror_state = state.clone();
    let mirror = tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(
            mirror_state.config.mirror_fetch_secs.max(1),
        ));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if let Err(error) = mirror_state.mirror.fetch().await {
                eprintln!("lodestar-server: mirror fetch: {error:#}");
            }
            mirror_state.broadcast(WsMessage::ChannelsUpdated(channels(&mirror_state).await));
        }
    });
    let agents = tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(state.config.poll_secs.max(1)));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            poll_once(&state).await;
        }
    });
    vec![mirror, agents]
}
