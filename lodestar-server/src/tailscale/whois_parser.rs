//! Parses `tailscale whois --json <ip>`.

use anyhow::Context;
use serde::Deserialize;

use super::Identity;

/// `LoginName` Tailscale reports for tagged devices (they have no user).
const TAGGED_LOGIN: &str = "tagged-devices";

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawWhois {
    node: RawNode,
    #[serde(default)]
    user_profile: Option<RawProfile>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawNode {
    #[serde(default)]
    name: String,
    #[serde(default)]
    computed_name: String,
    #[serde(default)]
    tags: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawProfile {
    #[serde(default)]
    login_name: String,
}

/// Parses `tailscale whois --json <ip>`.
pub fn parse_whois(json: &str) -> anyhow::Result<Identity> {
    let raw: RawWhois = serde_json::from_str(json).context("parsing tailscale whois")?;
    let node = if raw.node.computed_name.is_empty() {
        raw.node
            .name
            .split('.')
            .next()
            .unwrap_or_default()
            .to_string()
    } else {
        raw.node.computed_name
    };
    let login = raw
        .user_profile
        .map(|profile| profile.login_name)
        .filter(|login| !login.is_empty() && login != TAGGED_LOGIN);
    Ok(Identity {
        login,
        tags: raw.node.tags.unwrap_or_default(),
        node,
    })
}
