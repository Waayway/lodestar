use super::agents::Script;

/// Seconds laptop-2 stays offline.
const LAPTOP_2_OFFLINE_SECS: f64 = 120.0;

/// One row of the demo host list.
pub(super) struct DemoHost {
    pub(super) name: &'static str,
    pub(super) system: &'static str,
    pub(super) channel: &'static str,
    /// Commits behind the channel head at start.
    pub(super) behind: usize,
    pub(super) script: Script,
    /// Offline for this many seconds (unscaled) after start.
    pub(super) offline_for: Option<f64>,
    /// Online in Tailscale but nothing answers on the agent port.
    pub(super) unreachable: bool,
}

const fn host(
    name: &'static str,
    system: &'static str,
    channel: &'static str,
    behind: usize,
) -> DemoHost {
    DemoHost {
        name,
        system,
        channel,
        behind,
        script: Script::Succeed,
        offline_for: None,
        unreachable: false,
    }
}

pub(super) const DEMO_HOSTS: [DemoHost; 7] = [
    DemoHost {
        script: Script::Dirty,
        ..host("laptop-1", "aarch64-darwin", "unstable", 3)
    },
    DemoHost {
        offline_for: Some(LAPTOP_2_OFFLINE_SECS),
        ..host("laptop-2", "aarch64-darwin", "unstable", 5)
    },
    host("desktop", "x86_64-linux", "unstable", 2),
    DemoHost {
        unreachable: true,
        ..host("workstation", "x86_64-linux", "unstable", 1)
    },
    host("server-1", "x86_64-linux", "stable", 0),
    host("vpn", "x86_64-linux", "stable", 1),
    DemoHost {
        script: Script::FailFirst,
        ..host("media", "x86_64-linux", "stable", 1)
    },
];
