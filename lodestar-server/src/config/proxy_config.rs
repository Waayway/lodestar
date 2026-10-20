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
    /// The public URL, parsed and validated: `https://host[:port]` written with `://`, no
    /// userinfo, no query, no fragment and no path beyond an optional `/`. Plain http is
    /// refused: the session and login cookies are `__Host-…; Secure`, which a browser never
    /// keeps over http, so login could not work.
    pub fn parse_public_url(&self) -> anyhow::Result<reqwest::Url> {
        let text = self.public_url.trim();
        let url = reqwest::Url::parse(text)
            .map_err(|error| anyhow::anyhow!("proxy.public_url {text:?} is not a URL: {error}"))?;
        let written_with_authority = text
            .get(url.scheme().len()..)
            .is_some_and(|rest| rest.starts_with("://"));
        if url.scheme() != "https" || url.host().is_none() || !written_with_authority {
            anyhow::bail!("proxy.public_url {text:?} must be an https://host[:port] URL");
        }
        if !url.username().is_empty() || url.password().is_some() {
            anyhow::bail!("proxy.public_url {text:?} must not contain a user name or password");
        }
        if url.query().is_some() || url.fragment().is_some() {
            anyhow::bail!("proxy.public_url {text:?} must not contain a query or fragment");
        }
        if url.path() != "/" {
            anyhow::bail!("proxy.public_url {text:?} must not contain a path");
        }
        Ok(url)
    }

    /// `scheme://host[:port]` of the public URL as a browser serializes an origin: lowercase,
    /// default port dropped, no trailing slash. Empty if the URL is invalid (`check_proxy`
    /// refuses such a configuration at startup).
    pub fn public_origin(&self) -> String {
        self.parse_public_url()
            .map(|url| url.origin().ascii_serialization())
            .unwrap_or_default()
    }
}
