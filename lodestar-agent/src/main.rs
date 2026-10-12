use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use lodestar_agent::{apply, config};
use lodestar_proto::ApplyState;

#[derive(Parser)]
#[command(name = "lodestar-agent", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the agent daemon.
    Serve {
        #[arg(long)]
        config: PathBuf,
    },
    /// Run one apply (started by the daemon, outside its process).
    Apply {
        #[arg(long)]
        config: PathBuf,
        id: String,
        commit: String,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("lodestar-agent: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    match cli.command {
        Command::Serve {
            config: config_path,
        } => {
            let config = config::load(&config_path)?;
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(lodestar_agent::daemon::serve(config, config_path))?;
            Ok(ExitCode::FAILURE)
        }
        Command::Apply {
            config: config_path,
            id,
            commit,
        } => {
            let config = config::load(&config_path)?;
            let record = apply::run_apply(&config, &id, &commit);
            Ok(match record.state {
                ApplyState::Ok | ApplyState::Skipped => ExitCode::SUCCESS,
                _ => ExitCode::FAILURE,
            })
        }
    }
}
