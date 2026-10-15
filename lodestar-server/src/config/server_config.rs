use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::Context;
use lodestar_proto::PromoteChannels;
use serde::{Deserialize, Serialize};

use super::{AccessConfig, OnlineSource};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ServerConfig {
    pub listen: SocketAddr,
    pub agent_port: u16,
    pub hosts_file: PathBuf,
    /// `links.json` (service links); absent means no links.
    #[serde(default)]
    pub links_file: Option<PathBuf>,
    pub repo_url: String,
    pub push_url: String,
    #[serde(default)]
    pub deploy_key_file: Option<PathBuf>,
    pub state_dir: PathBuf,
    pub access: AccessConfig,
    pub online: OnlineSource,
    #[serde(default = "default_mirror_fetch_secs")]
    pub mirror_fetch_secs: u64,
    #[serde(default = "default_poll_secs")]
    pub poll_secs: u64,
    #[serde(default = "default_tailscale_bin")]
    pub tailscale_bin: PathBuf,
    /// Promote pair; absent means `unstable` -> `stable`, `null` disables promote.
    #[serde(default = "default_promote")]
    pub promote: Option<PromoteChannels>,
}

fn default_mirror_fetch_secs() -> u64 {
    300
}
fn default_poll_secs() -> u64 {
    60
}
fn default_promote() -> Option<PromoteChannels> {
    Some(PromoteChannels::default())
}
fn default_tailscale_bin() -> PathBuf {
    PathBuf::from("tailscale")
}

impl ServerConfig {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("parsing config {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_and_tags() {
        let config: ServerConfig = serde_json::from_str(
            r#"{"listen":"127.0.0.1:8080","agent_port":7461,"hosts_file":"/h.json",
            "repo_url":"u","push_url":"p","state_dir":"/s",
            "access":{"kind":"tailscale","allowed_users":["a@github"],"allowed_tags":["tag:lodestar"]},
            "online":{"kind":"static","addresses":{"h":"127.0.0.1"}}}"#,
        )
        .unwrap();
        assert_eq!((config.mirror_fetch_secs, config.poll_secs), (300, 60));
        assert_eq!(config.tailscale_bin, PathBuf::from("tailscale"));
        assert_eq!(config.deploy_key_file, None);
        assert_eq!(config.links_file, None);
        assert_eq!(config.promote, Some(PromoteChannels::default()));
        assert!(matches!(config.access, AccessConfig::Tailscale { .. }));
        let disabled: AccessConfig = serde_json::from_str(r#"{"kind":"disabled"}"#).unwrap();
        assert_eq!(disabled, AccessConfig::Disabled);
    }

    #[test]
    fn promote_null_disables_and_pair_is_configurable() {
        let base = r#""listen":"127.0.0.1:8080","agent_port":7461,"hosts_file":"/h.json",
            "repo_url":"u","push_url":"p","state_dir":"/s","access":{"kind":"disabled"},
            "online":{"kind":"tailscale"}"#;
        let off: ServerConfig =
            serde_json::from_str(&format!("{{{base},\"promote\":null}}")).unwrap();
        assert_eq!(off.promote, None);
        let custom: ServerConfig = serde_json::from_str(&format!(
            "{{{base},\"promote\":{{\"from\":\"main\",\"to\":\"release\"}}}}"
        ))
        .unwrap();
        assert_eq!(
            custom.promote,
            Some(PromoteChannels {
                from: "main".into(),
                to: "release".into()
            })
        );
    }
}
