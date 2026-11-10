use std::collections::HashMap;
use std::net::IpAddr;

use anyhow::Context;

use super::{Identity, PeerInfo, StaticOnline, TailscaleCli};
use crate::config::{ManagedHost, OnlineSource, check_tailscale_identities};

#[async_trait::async_trait]
pub trait OnlineProvider: Send + Sync {
    /// The tailnet node of each managed host that has one, keyed by lowercased host name.
    async fn peers(&self) -> HashMap<String, PeerInfo>;
    async fn whois(&self, ip: IpAddr) -> Option<Identity>;
}

/// Builds the provider selected by the config. With Tailscale, every host needs a valid,
/// unique Tailscale identity (see `ManagedHost`); the server does not start otherwise.
pub fn provider_from_config(
    source: &OnlineSource,
    tailscale_bin: &std::path::Path,
    hosts: &[ManagedHost],
) -> anyhow::Result<std::sync::Arc<dyn OnlineProvider>> {
    Ok(match source {
        OnlineSource::Tailscale => {
            check_tailscale_identities(hosts).context("hosts file")?;
            std::sync::Arc::new(TailscaleCli::new(
                tailscale_bin.to_path_buf(),
                hosts.to_vec(),
            ))
        }
        OnlineSource::Static { addresses } => std::sync::Arc::new(StaticOnline::new(addresses)?),
    })
}
