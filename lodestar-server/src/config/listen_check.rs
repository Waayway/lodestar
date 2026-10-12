use std::net::IpAddr;

use super::{AccessConfig, ServerConfig};

/// True for Tailscale addresses: `100.64.0.0/10` and `fd7a:115c:a1e0::/48`.
pub fn is_tailscale_ip(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            octets[0] == 100 && (octets[1] & 0xc0) == 64
        }
        IpAddr::V6(v6) => {
            let segments = v6.segments();
            segments[0] == 0xfd7a && segments[1] == 0x115c && segments[2] == 0xa1e0
        }
    }
}

impl ServerConfig {
    /// With Tailscale access control the dashboard must listen only on a Tailscale address:
    /// on any other address `whois` can't vouch for the peer.
    pub fn check_listen(&self) -> anyhow::Result<()> {
        if matches!(self.access, AccessConfig::Tailscale { .. })
            && !is_tailscale_ip(self.listen.ip())
        {
            anyhow::bail!(
                "listen address {} is not a Tailscale address (100.64.0.0/10 or fd7a:115c:a1e0::/48); \
                 with Tailscale access control the dashboard must listen only on the tailnet",
                self.listen
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tailscale_ips() {
        for ip in [
            "100.64.0.1",
            "100.127.255.254",
            "fd7a:115c:a1e0::1",
            "::ffff:100.100.1.1",
        ] {
            assert!(is_tailscale_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "100.63.255.255",
            "100.128.0.1",
            "127.0.0.1",
            "0.0.0.0",
            "::",
            "fd7a:115c:a1e1::1",
        ] {
            assert!(!is_tailscale_ip(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn listen_must_be_tailscale_with_tailscale_access() {
        let mut config: ServerConfig = serde_json::from_str(
            r#"{"listen":"0.0.0.0:8080","agent_port":7461,"hosts_file":"/h.json",
            "repo_url":"u","push_url":"p","state_dir":"/s",
            "access":{"kind":"tailscale"},"online":{"kind":"tailscale"}}"#,
        )
        .unwrap();
        let error = config.check_listen().unwrap_err().to_string();
        assert!(error.contains("not a Tailscale address"), "{error}");
        config.listen = "100.100.1.2:8080".parse().unwrap();
        config.check_listen().unwrap();
        config.listen = "127.0.0.1:8080".parse().unwrap();
        config.access = AccessConfig::Disabled;
        config.check_listen().unwrap();
    }
}
