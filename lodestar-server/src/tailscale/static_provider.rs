use std::collections::HashMap;
use std::net::IpAddr;

use anyhow::Context;

use super::{Identity, OnlineProvider, PeerInfo};

/// A static address is an IP, or (tests/dev only) `ip:port` when the agent listens on a
/// non-default port; see `poller::agent_base`.
pub fn parse_static_addr(text: &str) -> Option<IpAddr> {
    text.parse::<IpAddr>().ok().or_else(|| {
        text.parse::<std::net::SocketAddr>()
            .ok()
            .map(|address| address.ip())
    })
}

/// Dev/test source: every configured host is always online.
pub struct StaticOnline {
    peers: HashMap<String, PeerInfo>,
}

impl StaticOnline {
    pub fn new(addresses: &HashMap<String, String>) -> anyhow::Result<Self> {
        let mut peers = HashMap::new();
        for (name, address) in addresses {
            let host = name.to_lowercase();
            let ip = parse_static_addr(address)
                .with_context(|| format!("static address for {name}: {address:?}"))?;
            peers.insert(
                host.clone(),
                PeerInfo {
                    host,
                    online: true,
                    last_seen: None,
                    os: String::new(),
                    addr: Some(ip),
                },
            );
        }
        Ok(Self { peers })
    }
}

#[async_trait::async_trait]
impl OnlineProvider for StaticOnline {
    async fn peers(&self) -> HashMap<String, PeerInfo> {
        self.peers.clone()
    }
    async fn whois(&self, _ip: IpAddr) -> Option<Identity> {
        None
    }
}
