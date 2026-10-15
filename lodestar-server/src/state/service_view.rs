use std::collections::HashMap;

use lodestar_proto::{ServiceCheck, ServiceState, ServiceView};

use super::{AppState, online_state};
use crate::tailscale::PeerInfo;

/// One view per link, sorted by `(group, title)`.
pub fn service_views(state: &AppState, peers: &HashMap<String, PeerInfo>) -> Vec<ServiceView> {
    let checks = state.checks.lock().unwrap().clone();
    let mut views: Vec<ServiceView> = state
        .links
        .iter()
        .map(|link| {
            let check = checks.get(&link.id).cloned().unwrap_or(ServiceCheck {
                state: if link.check.enable {
                    ServiceState::Pending
                } else {
                    ServiceState::Unchecked
                },
                reason: None,
                latency_ms: None,
                checked_at: None,
                since: None,
            });
            let host_online = state
                .hosts
                .iter()
                .any(|host| host.name == link.host)
                .then(|| online_state(state, &link.host, peers));
            ServiceView {
                id: link.id.clone(),
                title: link.title.clone(),
                url: link.url.clone(),
                group: link.group.clone(),
                description: link.description.clone(),
                icon: link.icon.clone(),
                host: link.host.clone(),
                host_online,
                check,
            }
        })
        .collect();
    views.sort_by(|a, b| (&a.group, &a.title).cmp(&(&b.group, &b.title)));
    views
}
