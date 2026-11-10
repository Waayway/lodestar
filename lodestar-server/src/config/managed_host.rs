use std::collections::HashMap;
use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// One host of the fleet, as listed in `hosts.json`.
///
/// With `online.kind = "tailscale"` a host is bound to a tailnet node by `node_id` (the stable
/// node ID, `ID` in `tailscale status --json`) and/or `dns_name` (its MagicDNS name), never by
/// the HostName a device reports about itself: any device can claim any HostName.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ManagedHost {
    pub name: String,
    pub system: String,
    pub user: String,
    pub channel: String,
    pub os: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dns_name: Option<String>,
}

/// Reads `hosts.json`. Host names are compared case-insensitively everywhere, so two that
/// differ only in case are refused.
pub fn load_hosts(path: &Path) -> anyhow::Result<Vec<ManagedHost>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading hosts file {}", path.display()))?;
    let hosts: Vec<ManagedHost> = serde_json::from_str(&text)
        .with_context(|| format!("parsing hosts file {}", path.display()))?;
    let mut seen = HashMap::new();
    for host in &hosts {
        if let Some(other) = seen.insert(host.name.to_lowercase(), &host.name) {
            anyhow::bail!(
                "hosts file {}: hosts {other} and {} differ only in case",
                path.display(),
                host.name
            );
        }
    }
    Ok(hosts)
}

/// A MagicDNS name compared the way Tailscale reports it: lowercase, without the final dot.
pub fn normalize_dns_name(name: &str) -> String {
    name.trim_end_matches('.').to_ascii_lowercase()
}

/// Checks that every host has a Tailscale identity (`node_id` or `dns_name`), that each is
/// well-formed, and that no two hosts share one.
pub fn check_tailscale_identities(hosts: &[ManagedHost]) -> anyhow::Result<()> {
    let mut node_ids = HashMap::new();
    let mut dns_names = HashMap::new();
    for host in hosts {
        let name = &host.name;
        anyhow::ensure!(
            host.node_id.is_some() || host.dns_name.is_some(),
            "host {name} has no Tailscale identity: set node_id (the node's stable ID) or \
             dns_name (its full MagicDNS name)"
        );
        if let Some(node_id) = &host.node_id {
            anyhow::ensure!(
                valid_node_id(node_id),
                "host {name}: node_id {node_id:?} is not a Tailscale stable node ID"
            );
            if let Some(other) = node_ids.insert(node_id.clone(), name) {
                anyhow::bail!("hosts {other} and {name} have the same node_id {node_id:?}");
            }
        }
        if let Some(dns_name) = &host.dns_name {
            let normalized = normalize_dns_name(dns_name);
            anyhow::ensure!(
                valid_dns_name(&normalized),
                "host {name}: dns_name {dns_name:?} is not a full MagicDNS name \
                 (like laptop.example.ts.net)"
            );
            if let Some(other) = dns_names.insert(normalized, name) {
                anyhow::bail!("hosts {other} and {name} have the same dns_name {dns_name:?}");
            }
        }
    }
    Ok(())
}

/// Letters and digits (`nAbCdEf1CNTRL`), `-` and `_` after the first.
fn valid_node_id(node_id: &str) -> bool {
    node_id.len() <= 64
        && node_id
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_alphanumeric())
        && node_id
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || char == '-' || char == '_')
}

/// At least two labels of letters, digits and inner `-` (already normalized).
fn valid_dns_name(name: &str) -> bool {
    let labels: Vec<&str> = name.split('.').collect();
    name.len() <= 253
        && labels.len() >= 2
        && labels.iter().all(|label| {
            (1..=63).contains(&label.len())
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .chars()
                    .all(|char| char.is_ascii_lowercase() || char.is_ascii_digit() || char == '-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_load() {
        let dir = std::env::temp_dir().join(format!("lodestar-hosts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hosts.json");
        std::fs::write(
            &path,
            r#"[{"name":"a","system":"x86_64-linux","user":"u","channel":"main","os":"nixos"}]"#,
        )
        .unwrap();
        let hosts = load_hosts(&path).unwrap();
        assert_eq!(hosts[0].name, "a");
        std::fs::remove_dir_all(&dir).ok();
    }
}
