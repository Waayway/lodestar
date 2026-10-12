use std::collections::HashMap;

use lodestar_proto::{ActivationEvent, HostView, OnlineState};

use super::AppState;
use crate::config::ManagedHost;
use crate::db;
use crate::tailscale::PeerInfo;

pub async fn host_view(state: &AppState, host: &ManagedHost) -> HostView {
    let peers = state.online.peers().await;
    host_view_with(state, host, &peers).await
}

/// `host_view` with a `peers()` result the caller already has.
pub(crate) async fn host_view_with(
    state: &AppState,
    host: &ManagedHost,
    peers: &HashMap<String, PeerInfo>,
) -> HostView {
    let peer = peers.get(&host.name.to_lowercase());
    let online = match peer {
        Some(peer) if peer.online => {
            let reachable = state.reachable.lock().unwrap().get(&host.name).copied();
            if reachable == Some(true) {
                OnlineState::On
            } else {
                OnlineState::AgentUnreachable
            }
        }
        _ => OnlineState::Off,
    };
    let stored = db::load_status(&state.db, &host.name)
        .await
        .unwrap_or_else(|error| {
            eprintln!(
                "lodestar-server: loading status of {}: {error:#}",
                host.name
            );
            None
        });
    let (status, last_ok_at) = match stored {
        Some((status, received_at)) => (Some(status), received_at),
        None => (None, None),
    };
    let behind = match &status {
        Some(status) => {
            state
                .mirror
                .behind_count(&status.running, &host.channel)
                .await
        }
        None => None,
    };
    let desired_apply_id = db::desired(&state.db, &host.name)
        .await
        .ok()
        .flatten()
        .map(|desired| desired.apply_id);
    let last_activation = db::last_audit(&state.db, &host.name, "activation")
        .await
        .ok()
        .flatten()
        .and_then(|event| serde_json::from_value::<ActivationEvent>(event.details).ok());
    HostView {
        name: host.name.clone(),
        os: host.os.clone(),
        channel: host.channel.clone(),
        online,
        last_seen: peer.and_then(|peer| peer.last_seen).or(last_ok_at),
        running_dirty: status
            .as_ref()
            .is_some_and(|status| status.running.ends_with("-dirty")),
        running: status.as_ref().map(|status| status.running.clone()),
        behind,
        checkout: status.as_ref().map(|status| status.checkout.clone()),
        last_apply: status.and_then(|status| status.apply),
        desired_apply_id,
        last_activation,
    }
}
