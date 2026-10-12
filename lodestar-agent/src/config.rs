use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// How the apply wrapper detaches the rebuild from the daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Launcher {
    SystemdRun,
    Detach,
    Inline,
}

/// Agent configuration, a JSON file passed as `--config <path>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentConfig {
    pub host: String,
    pub user: String,
    pub channel: String,
    pub flake_path: PathBuf,
    /// Flake output to build, e.g. `nixosConfigurations.<host>.config.system.build.toplevel`;
    /// defaults to that (or `darwinConfigurations.<host>.system` on darwin).
    #[serde(default)]
    pub flake_attr: Option<String>,
    pub repo_url: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub manager_host: String,
    #[serde(default)]
    pub listen: Option<SocketAddr>,
    #[serde(default = "default_state_dir")]
    pub state_dir: PathBuf,
    #[serde(default)]
    pub rebuild_command: Option<Vec<String>>,
    /// Command whose output (with `--configuration-revision` appended) is the
    /// running revision; defaults to the system's `nixos-version`/`darwin-version`.
    #[serde(default)]
    pub revision_command: Option<Vec<String>>,
    pub launcher: Launcher,
    /// Binary the launcher starts for an apply; defaults to the current executable.
    #[serde(default)]
    pub agent_exe: Option<PathBuf>,
    /// "linux" or "darwin"
    pub os: String,
}

fn default_port() -> u16 {
    7461
}

fn default_state_dir() -> PathBuf {
    PathBuf::from("/var/lib/lodestar-agent")
}

impl AgentConfig {
    /// The flake output `nix build` builds for this host.
    pub fn flake_attr(&self) -> String {
        match &self.flake_attr {
            Some(attr) => attr.clone(),
            None if self.os == "darwin" => format!("darwinConfigurations.{}.system", self.host),
            None => format!(
                "nixosConfigurations.{}.config.system.build.toplevel",
                self.host
            ),
        }
    }
}

pub fn load(path: &Path) -> anyhow::Result<AgentConfig> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading config {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("parsing config {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(os: &str, attr: Option<&str>) -> AgentConfig {
        serde_json::from_value(serde_json::json!({
            "host": "box", "user": "alice", "channel": "main", "flake_path": "/home/alice/infra",
            "repo_url": "https://example.com/infra.git", "manager_host": "manager",
            "launcher": "inline", "os": os, "flake_attr": attr,
        }))
        .unwrap()
    }

    #[test]
    fn flake_attr_defaults_per_os_and_can_be_overridden() {
        assert_eq!(
            cfg("linux", None).flake_attr(),
            "nixosConfigurations.box.config.system.build.toplevel"
        );
        assert_eq!(
            cfg("darwin", None).flake_attr(),
            "darwinConfigurations.box.system"
        );
        assert_eq!(
            cfg("linux", Some("packages.x86_64-linux.box")).flake_attr(),
            "packages.x86_64-linux.box"
        );
    }
}
