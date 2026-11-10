use std::net::IpAddr;

/// A tailnet node as `tailscale status` reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct PeerInfo {
    /// Lowercased HostName: what the device calls itself. Not unique, not trusted.
    pub host: String,
    /// Stable node ID (`ID`); empty when not reported.
    pub node_id: String,
    /// MagicDNS name, lowercase without the final dot; empty when not reported.
    pub dns_name: String,
    pub online: bool,
    /// Unix seconds; `None` when Tailscale has never seen the node.
    pub last_seen: Option<i64>,
    pub os: String,
    pub addr: Option<IpAddr>,
}
