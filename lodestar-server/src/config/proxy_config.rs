use std::net::{IpAddr, SocketAddr};

use serde::{Deserialize, Serialize};

use super::OidcConfig;

/// The router entrance: a second listener reached through a trusted reverse proxy.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ProxyConfig {
    pub listen: SocketAddr,
    pub trusted_proxies: Vec<IpAddr>,
    pub public_url: String,
    pub oidc: OidcConfig,
}

impl ProxyConfig {
    /// `scheme://host[:port]` of the public URL, without path or trailing slash.
    pub fn public_origin(&self) -> String {
        let url = self.public_url.trim();
        let (scheme, rest) = url.split_once("://").unwrap_or(("", url));
        let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
        format!("{scheme}://{authority}")
    }
}
