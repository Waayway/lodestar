use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use clap::{Parser, Subcommand};
use lodestar_server::config::{ServerConfig, load_hosts, load_links};
use lodestar_server::{demo, server, tailscale};

#[derive(Parser)]
#[command(
    name = "lodestar-server",
    version,
    args_conflicts_with_subcommands = true
)]
struct Cli {
    /// Server configuration (JSON); same as `lodestar-server serve --config`.
    #[arg(long)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Runs the Lodestar server.
    Serve {
        /// Server configuration (JSON).
        #[arg(long)]
        config: PathBuf,
    },
    /// Runs the real server and dashboard against simulated hosts.
    Demo {
        /// Dashboard address. Access control is off in demo mode.
        #[arg(long, default_value = "127.0.0.1:8080")]
        listen: SocketAddr,
        /// Time multiplier for the simulation (applies, polling, new commits).
        #[arg(long, default_value_t = 1.0)]
        speed: f64,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let command = match (cli.command, cli.config) {
        (Some(command), _) => command,
        (None, Some(config)) => Command::Serve { config },
        (None, None) => {
            eprintln!("lodestar-server: give --config <path>, or a subcommand (see --help)");
            return ExitCode::from(2);
        }
    };
    let result = tokio::runtime::Runtime::new()
        .context("starting the tokio runtime")
        .and_then(|runtime| runtime.block_on(run(command)));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lodestar-server: {error:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run(command: Command) -> anyhow::Result<()> {
    match command {
        Command::Serve {
            config: config_path,
        } => {
            let config = ServerConfig::load(&config_path)?;
            let hosts = load_hosts(&config.hosts_file)?;
            let links = match &config.links_file {
                Some(path) => load_links(path)?,
                None => Vec::new(),
            };
            let online = tailscale::provider_from_config(&config.online, &config.tailscale_bin)?;
            server::start(config, hosts, links, online, false)
                .await?
                .wait()
                .await
        }
        Command::Demo { listen, speed } => {
            let handle = demo::start(listen, speed).await?;
            eprintln!("lodestar-server: demo dashboard at http://{}/", handle.addr);
            // On Ctrl-C the handle is dropped, which stops the demo and removes its temp dir.
            tokio::select! {
                result = handle.wait() => result,
                result = tokio::signal::ctrl_c() => result.context("waiting for Ctrl-C"),
            }
        }
    }
}
