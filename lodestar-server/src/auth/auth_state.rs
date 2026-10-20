use std::time::Duration;

use anyhow::Context;

use super::{OidcClient, PendingLogins};
use crate::config::ProxyConfig;

/// How often an open WebSocket of a logged-in viewer checks that its session still exists.
pub const SESSION_RECHECK: Duration = Duration::from_secs(60);

/// Everything the router entrance needs to log people in and recognise them.
pub struct AuthState {
    pub config: ProxyConfig,
    pub oidc: OidcClient,
    pub pending: PendingLogins,
    pub session_recheck: Duration,
}

impl AuthState {
    pub fn new(config: ProxyConfig) -> anyhow::Result<Self> {
        let public_url = config.parse_public_url()?;
        let oidc = OidcClient::new(config.oidc.clone(), public_url.as_str())
            .context("setting up the OpenID Connect client")?;
        Ok(AuthState {
            config,
            oidc,
            pending: PendingLogins::new(),
            session_recheck: SESSION_RECHECK,
        })
    }
}
