use std::net::IpAddr;
use std::time::Duration;

/// A simulated tailnet peer: one demo host.
pub struct DemoPeer {
    pub host: String,
    /// Tailscale's OS name (`macOS`, `linux`).
    pub os: String,
    pub addr: IpAddr,
    /// Off for this long after the demo starts (already scaled by speed).
    pub offline_for: Option<Duration>,
}
