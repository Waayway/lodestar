use std::net::IpAddr;

/// A tailnet node as `tailscale status` reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct PeerInfo {
    pub host: String,
    pub online: bool,
    /// Unix seconds; `None` when Tailscale has never seen the node.
    pub last_seen: Option<i64>,
    pub os: String,
    pub addr: Option<IpAddr>,
}
