//! Parses `tailscale status --json`.

use std::collections::HashMap;
use std::net::IpAddr;

use anyhow::Context;
use serde::Deserialize;

use super::PeerInfo;
use crate::config::normalize_dns_name;

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
    #[serde(rename = "ID", default)]
    id: String,
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

/// Every node of `tailscale status --json`, this node (`Self`) included, in no particular
/// order. Nothing is keyed by HostName: see `match_peers` for how managed hosts find theirs.
pub fn parse_status(json: &str) -> anyhow::Result<Vec<PeerInfo>> {
    let raw: RawStatus = serde_json::from_str(json).context("parsing tailscale status")?;
    Ok(raw
        .this
        .into_iter()
        .chain(raw.peer.into_iter().flat_map(|peers| peers.into_values()))
        .map(|peer| {
            let addr = peer
                .tailscale_ips
                .iter()
                .find(|ip| ip.is_ipv4())
                .or_else(|| peer.tailscale_ips.first())
                .copied();
            PeerInfo {
                host: peer.host_name.to_lowercase(),
                node_id: peer.id,
                dns_name: normalize_dns_name(&peer.dns_name),
                online: peer.online,
                last_seen: peer.last_seen.as_deref().and_then(parse_last_seen),
                os: peer.os,
                addr,
            }
        })
        .collect())
}

fn parse_last_seen(text: &str) -> Option<i64> {
    let timestamp = chrono::DateTime::parse_from_rfc3339(text).ok()?.timestamp();
    // Go's zero time ("0001-01-01T00:00:00Z") means never seen.
    (timestamp > 0).then_some(timestamp)
}
