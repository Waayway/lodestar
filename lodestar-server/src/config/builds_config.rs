use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Building every Linux host's system once on a builder over `ssh-ng://` and publishing it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct BuildsConfig {
    pub enable: bool,
    /// The builder's store URL (`ssh-ng://user@host`).
    pub store: String,
    pub ssh_key_file: Option<PathBuf>,
    /// Command run to publish the built paths; the first element is the program.
    pub publish_command: Vec<String>,
    pub timeout_secs: u64,
}

impl Default for BuildsConfig {
    fn default() -> Self {
        Self {
            enable: false,
            store: String::new(),
            ssh_key_file: None,
            publish_command: vec!["cache-publish".into()],
            timeout_secs: 7200,
        }
    }
}

impl BuildsConfig {
    /// Validates an enabled config; a disabled one is never used, so it is not checked.
    pub fn check(&self) -> anyhow::Result<()> {
        if !self.enable {
            return Ok(());
        }
        if self.store.is_empty() {
            anyhow::bail!("builds.store must be set while builds are enabled");
        }
        if !(self.store.starts_with("ssh-ng://") || self.store.starts_with("ssh://")) {
            anyhow::bail!(
                "builds.store {:?} must start with ssh-ng:// or ssh://",
                self.store
            );
        }
        if self.publish_command.is_empty() || self.publish_command[0].is_empty() {
            anyhow::bail!("builds.publish_command must name a program");
        }
        if self.timeout_secs == 0 {
            anyhow::bail!("builds.timeout_secs must be greater than 0");
        }
        Ok(())
    }
}
