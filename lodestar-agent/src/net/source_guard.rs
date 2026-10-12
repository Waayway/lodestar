use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::tailscale_addresses::tailscale_addrs;

const CACHE_TTL: Duration = Duration::from_secs(60);

/// The manager's addresses as seen by this agent. When the manager name
/// resolves to loopback, the manager is this machine (NixOS maps the own
/// hostname to 127.0.0.2 in /etc/hosts), and the server reaches the agent
/// from this machine's own Tailscale addresses, so those count too.
pub fn manager_addrs(resolved: Vec<IpAddr>, own_tailscale: &[IpAddr]) -> Vec<IpAddr> {
    let mut ips: Vec<IpAddr> = resolved.into_iter().map(|ip| ip.to_canonical()).collect();
    if ips.iter().any(|ip| ip.is_loopback()) {
        ips.extend(own_tailscale.iter().map(|ip| ip.to_canonical()));
    }
    ips
}

/// Allows only the addresses the manager host currently resolves to.
pub struct SourceGuard {
    host: String,
    cache: Mutex<Option<(Instant, Vec<IpAddr>)>>,
}

impl SourceGuard {
    pub fn new(host: String) -> Self {
        SourceGuard {
            host,
            cache: Mutex::new(None),
        }
    }

    pub async fn allows(&self, ip: IpAddr) -> bool {
        let ip = ip.to_canonical();
        let cached = {
            let cache = self.cache.lock().unwrap();
            cache
                .as_ref()
                .map(|(resolved_at, ips)| (resolved_at.elapsed() < CACHE_TTL, ips.clone()))
        };
        let ips = match cached {
            Some((true, ips)) => ips,
            stale => match tokio::net::lookup_host((self.host.as_str(), 0)).await {
                Ok(addresses) => {
                    let ips = manager_addrs(
                        addresses.map(|address| address.ip()).collect(),
                        &tailscale_addrs(),
                    );
                    *self.cache.lock().unwrap() = Some((Instant::now(), ips.clone()));
                    ips
                }
                // Keep using the last answer if DNS is briefly unavailable.
                Err(_) => stale.map(|(_, ips)| ips).unwrap_or_default(),
            },
        };
        ips.contains(&ip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manager_addrs_adds_own_tailscale_addrs_when_manager_is_this_host() {
        let own: Vec<IpAddr> = vec!["100.64.0.10".parse().unwrap()];
        let got = manager_addrs(vec!["127.0.0.2".parse().unwrap()], &own);
        assert!(got.contains(&"100.64.0.10".parse().unwrap()));
    }

    #[test]
    fn manager_addrs_unchanged_for_a_remote_manager() {
        let own: Vec<IpAddr> = vec!["100.95.164.9".parse().unwrap()];
        let remote: IpAddr = "100.64.0.10".parse().unwrap();
        assert_eq!(manager_addrs(vec![remote], &own), vec![remote]);
    }
}
