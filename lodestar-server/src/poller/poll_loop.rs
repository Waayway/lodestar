use std::time::Duration;

use lodestar_proto::WsMessage;
use tokio::time::MissedTickBehavior;

use super::host_poll::poll_once;
use super::service_check::{CHECK_INTERVAL, Probers, check_round};
use crate::state::{AppState, channels};

/// Background loops: mirror fetch every `mirror_fetch_secs` (then `ChannelsUpdated`) and
/// agent polling every `poll_secs`, and service link checks every `CHECK_INTERVAL`. All run
/// once immediately; the checks never wait for the host poller.
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
    let mut tasks = vec![mirror];
    match Probers::new() {
        Ok(probers) => {
            let check_state = state.clone();
            tasks.push(tokio::spawn(async move {
                let mut tick = tokio::time::interval(CHECK_INTERVAL);
                tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
                loop {
                    tick.tick().await;
                    check_round(&check_state, &probers).await;
                }
            }));
        }
        Err(error) => eprintln!("lodestar-server: service checks disabled: {error:#}"),
    }
    let agents = tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(state.config.poll_secs.max(1)));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            poll_once(&state).await;
        }
    });
    tasks.push(agents);
    tasks
}
