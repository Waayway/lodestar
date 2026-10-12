//! Parses `tailscale status --json`.

use std::collections::HashMap;
use std::net::IpAddr;

use anyhow::Context;
use serde::Deserialize;

use super::PeerInfo;

#[derive(Deserialize)]
struct RawStatus {
    #[serde(rename = "Self")]
    this: Option<RawPeer>,
    #[serde(rename = "Peer", default)]
    peer: Option<HashMap<String, RawPeer>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawPeer {
    #[serde(default)]
    host_name: String,
    #[serde(rename = "DNSName", default)]
    dns_name: String,
    #[serde(default)]
    online: bool,
    #[serde(default)]
    last_seen: Option<String>,
    #[serde(rename = "OS", default)]
    os: String,
    #[serde(rename = "TailscaleIPs", default)]
    tailscale_ips: Vec<IpAddr>,
}

/// A parsed peer with what collision resolution needs: whether it is this node, and its
/// DNSName.
type Candidate = (bool, String, PeerInfo);

/// Parses `tailscale status --json`, including this node. Key = lowercased HostName.
///
/// HostName is not unique in a tailnet. Collisions resolve deterministically:
/// Self wins, else online beats offline, then the later `last_seen`, then the
/// lexicographically smaller DNSName. A warning is logged for each collision.
pub fn parse_status(json: &str) -> anyhow::Result<HashMap<String, PeerInfo>> {
    let raw: RawStatus = serde_json::from_str(json).context("parsing tailscale status")?;
    let candidates = raw.this.into_iter().map(|peer| (true, peer)).chain(
        raw.peer
            .into_iter()
            .flat_map(|peers| peers.into_values())
            .map(|peer| (false, peer)),
    );
    let mut out: HashMap<String, Candidate> = HashMap::new();
    for (is_self, peer) in candidates {
        let host = peer.host_name.to_lowercase();
        if host.is_empty() {
            continue;
        }
        let addr = peer
            .tailscale_ips
            .iter()
            .find(|ip| ip.is_ipv4())
            .or_else(|| peer.tailscale_ips.first())
            .copied();
        let info = PeerInfo {
            host: host.clone(),
            online: peer.online,
            last_seen: peer.last_seen.as_deref().and_then(parse_last_seen),
            os: peer.os,
            addr,
        };
        let new = (is_self, peer.dns_name, info);
        match out.remove(&host) {
            None => {
                out.insert(host, new);
            }
            Some(old) => {
                eprintln!(
                    "lodestar-server: duplicate tailscale HostName {host:?}; picking one deterministically"
                );
                out.insert(host, if wins(&new, &old) { new } else { old });
            }
        }
    }
    Ok(out
        .into_iter()
        .map(|(host, (_, _, info))| (host, info))
        .collect())
}

/// True if `challenger` should replace `incumbent` for the same host name.
fn wins(challenger: &Candidate, incumbent: &Candidate) -> bool {
    // Self first, then online, then later last_seen (None is oldest), then smaller DNSName.
    let key = |candidate: &Candidate| (candidate.0, candidate.2.online, candidate.2.last_seen);
    match key(challenger).cmp(&key(incumbent)) {
        std::cmp::Ordering::Equal => challenger.1 < incumbent.1,
        ordering => ordering.is_gt(),
    }
}

fn parse_last_seen(text: &str) -> Option<i64> {
    let timestamp = chrono::DateTime::parse_from_rfc3339(text).ok()?.timestamp();
    // Go's zero time ("0001-01-01T00:00:00Z") means never seen.
    (timestamp > 0).then_some(timestamp)
}
