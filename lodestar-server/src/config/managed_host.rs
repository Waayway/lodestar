use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// One host of the fleet, as listed in `hosts.json`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ManagedHost {
    pub name: String,
    pub system: String,
    pub user: String,
    pub channel: String,
    pub os: String,
}

/// Host names become part of a flake attribute path (`.#…Configurations."<name>"`), so every
/// caller that builds such a path must reject one that is not this. Checked once here, at
/// load, so every list of hosts in the server already satisfies it.
pub fn valid_host_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

pub fn load_hosts(path: &Path) -> anyhow::Result<Vec<ManagedHost>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading hosts file {}", path.display()))?;
    let hosts: Vec<ManagedHost> = serde_json::from_str(&text)
        .with_context(|| format!("parsing hosts file {}", path.display()))?;
    if let Some(host) = hosts.iter().find(|host| !valid_host_name(&host.name)) {
        anyhow::bail!(
            "hosts file {}: invalid host name {:?}",
            path.display(),
            host.name
        );
    }
    Ok(hosts)
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

    #[test]
    fn hosts_load_rejects_an_invalid_name() {
        let dir =
            std::env::temp_dir().join(format!("lodestar-hosts-invalid-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hosts.json");
        std::fs::write(
            &path,
            r#"[{"name":"a\"","system":"x86_64-linux","user":"u","channel":"main","os":"nixos"}]"#,
        )
        .unwrap();
        assert!(load_hosts(&path).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
