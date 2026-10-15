use futures::stream::{self, StreamExt};
use lodestar_proto::{ServiceCheck, WsMessage};

use super::{CHECK_CONCURRENCY, ProbeOutcome, Probers, probe};
use crate::clock;
use crate::config::Link;
use crate::state::{AppState, service_views};

/// Probes one link, keyed by its id.
async fn probe_link(probers: &Probers, link: Link) -> (String, ProbeOutcome) {
    let outcome = probe(probers, &link).await;
    (link.id, outcome)
}

/// Probes every enabled link, records the results and broadcasts the services when a state
/// or reason changed. Returns whether it did.
pub async fn check_round(state: &AppState, probers: &Probers) -> bool {
    // Owned links: a stream of borrowed `&Link` makes the round's future not `Send` for
    // `tokio::spawn` (rust-lang/rust#102211). Cloning a few links once a minute is cheap.
    let enabled: Vec<Link> = state
        .links
        .iter()
        .filter(|link| link.check.enable)
        .cloned()
        .collect();
    let results: Vec<(String, ProbeOutcome)> = stream::iter(enabled)
        .map(|link| probe_link(probers, link))
        .buffer_unordered(CHECK_CONCURRENCY)
        .collect()
        .await;
    let now = clock::now();
    let mut changed = false;
    {
        let mut checks = state.checks.lock().unwrap();
        for (id, outcome) in results {
            let previous = checks.get(&id);
            let state_changed = previous.is_none_or(|old| old.state != outcome.state);
            let reason_changed = previous.is_none_or(|old| old.reason != outcome.reason);
            changed |= state_changed || reason_changed;
            let since = match previous {
                Some(old) if !state_changed => old.since,
                _ => Some(now),
            };
            checks.insert(
                id,
                ServiceCheck {
                    state: outcome.state,
                    reason: outcome.reason,
                    latency_ms: outcome.latency_ms,
                    checked_at: Some(now),
                    since,
                },
            );
        }
    }
    if changed {
        let peers = state.online.peers().await;
        state.broadcast(WsMessage::ServicesUpdated(service_views(state, &peers)));
    }
    changed
}
