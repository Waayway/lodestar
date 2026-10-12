use std::collections::HashMap;
use std::net::IpAddr;

use super::{Identity, PeerInfo, StaticOnline, TailscaleCli};
use crate::config::OnlineSource;

#[async_trait::async_trait]
pub trait OnlineProvider: Send + Sync {
    async fn peers(&self) -> HashMap<String, PeerInfo>;
    async fn whois(&self, ip: IpAddr) -> Option<Identity>;
}

/// Builds the provider selected by the config.
pub fn provider_from_config(
    source: &OnlineSource,
    tailscale_bin: &std::path::Path,
) -> anyhow::Result<std::sync::Arc<dyn OnlineProvider>> {
    Ok(match source {
        OnlineSource::Tailscale => {
            std::sync::Arc::new(TailscaleCli::new(tailscale_bin.to_path_buf()))
        }
        OnlineSource::Static { addresses } => std::sync::Arc::new(StaticOnline::new(addresses)?),
    })
}
