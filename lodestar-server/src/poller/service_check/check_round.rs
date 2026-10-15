use futures::future::join_all;
use lodestar_proto::{ServiceCheck, WsMessage};

use super::{CHECK_CONCURRENCY, ProbeOutcome, Probers, probe};
use tokio::sync::Semaphore;

use crate::clock;
use crate::config::Link;
use crate::state::{AppState, service_views};

/// Probes one link once a concurrency permit is free.
async fn probe_link(permits: &Semaphore, probers: &Probers, link: &Link) -> (String, ProbeOutcome) {
    let _permit = permits.acquire().await.expect("semaphore is never closed");
    (link.id.clone(), probe(probers, link).await)
}

/// Probes every enabled link, records the results and broadcasts the services when a state
/// or reason changed. Returns whether it did.
pub async fn check_round(state: &AppState, probers: &Probers) -> bool {
    let permits = Semaphore::new(CHECK_CONCURRENCY);
    let results = join_all(
        state
            .links
            .iter()
            .filter(|link| link.check.enable)
            .map(|link| probe_link(&permits, probers, link)),
    )
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
