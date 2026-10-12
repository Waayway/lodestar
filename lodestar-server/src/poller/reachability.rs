//! Where a host's agent is, and whether it answered.

use std::collections::HashMap;
use std::net::SocketAddr;

use crate::config::{ManagedHost, OnlineSource, ServerConfig};
use crate::state::AppState;
use crate::tailscale::PeerInfo;

/// `host:port` of a host's agent, or `None` when Tailscale doesn't report it online with an
/// address. With `OnlineSource::Static`, an address that already has a port (tests/dev only)
/// is used as-is; otherwise `agent_port` is appended.
pub fn agent_base(
    config: &ServerConfig,
    host: &ManagedHost,
    peers: &HashMap<String, PeerInfo>,
) -> Option<String> {
    let peer = peers.get(&host.name.to_lowercase())?;
    let ip = peer.addr.filter(|_| peer.online)?;
    if let OnlineSource::Static { addresses } = &config.online
        && let Some(with_port) = addresses
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(&host.name))
            .and_then(|(_, address)| address.parse::<SocketAddr>().ok())
    {
        return Some(with_port.to_string());
    }
    Some(SocketAddr::new(ip, config.agent_port).to_string())
}

/// Remembers whether `host`'s agent answered and returns a log line when that changed:
/// the first failure (and the first after a success), and the first success after a failure.
/// An unchanged state logs nothing, so an unreachable agent doesn't fill the log every poll.
pub(super) fn reachability_change(
    state: &AppState,
    host: &str,
    result: Result<(), &anyhow::Error>,
) -> Option<String> {
    let previous = state
        .agent_up
        .lock()
        .unwrap()
        .insert(host.to_string(), result.is_ok());
    match (result, previous) {
        (Err(error), None | Some(true)) => {
            Some(format!("{host}: agent unreachable: {}", error.root_cause()))
        }
        (Ok(()), Some(false)) => Some(format!("{host}: agent reachable again")),
        _ => None,
    }
}
