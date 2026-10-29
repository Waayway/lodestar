use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "lodestar", about = "Talk to the Lodestar server")]
pub struct Cli {
    /// Lodestar server URL (e.g. `http://manager:8080`). Falls back to `server = "..."` in
    /// `$XDG_CONFIG_HOME/lodestar/config.toml` (default `~/.config/lodestar/config.toml`).
    #[arg(long, env = "LODESTAR_SERVER")]
    pub server: Option<String>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Table of all hosts.
    Status,
    /// Force a rebuild: apply the channel's current head, even if the host already runs it.
    Rebuild {
        /// Host to rebuild (omit when using --channel).
        host: Option<String>,
        /// Rebuild every host on this channel instead.
        #[arg(long, conflicts_with = "host")]
        channel: Option<String>,
        /// Wait for the apply to finish; exit 0 only if it succeeded. Keeps retrying while the
        /// server is unreachable (e.g. restarting because the apply switched its host).
        #[arg(long)]
        wait: bool,
        /// Give up waiting after this many seconds.
        #[arg(long, default_value_t = 900)]
        timeout: u64,
    },
    /// Print an apply's log (default: the host's last apply).
    Log {
        host: String,
        apply_id: Option<String>,
    },
    /// Fast-forward the server's promote target channel to the candidate from its source
    /// channel (the pair is configured on the server; `status` shows the channels).
    Promote {
        /// Actually promote (otherwise only show the candidate).
        #[arg(long)]
        yes: bool,
    },
    /// Tracked software, its latest upstream version and the last update run per flake
    /// input; `updates run` starts a run now.
    Updates {
        #[command(subcommand)]
        action: Option<UpdatesAction>,
    },
    /// Recent audit events.
    Audit {
        #[arg(long, default_value_t = 20)]
        limit: u64,
    },
}

#[derive(Subcommand)]
pub enum UpdatesAction {
    /// Check and bump the tracked flake inputs now.
    Run {
        /// Wait for the run to finish; exit 0 only if every input ended `pushed` or
        /// `no-change`.
        #[arg(long)]
        wait: bool,
        /// Give up waiting after this many seconds.
        #[arg(long, default_value_t = 1800)]
        timeout: u64,
    },
}
