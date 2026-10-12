use std::io::Write;
use std::time::Duration;

use anyhow::Result;

use super::{audit, log, promote, rebuild, status};
use crate::cli::Command;
use crate::client::Client;

/// Runs a command, writing results to `out`. Returns the process exit code.
pub async fn run(client: &Client, command: Command, out: &mut impl Write) -> Result<i32> {
    match command {
        Command::Status => status::status(client, out).await,
        Command::Rebuild {
            host,
            channel,
            wait,
            timeout,
        } => {
            rebuild::rebuild(
                client,
                host,
                channel,
                wait,
                Duration::from_secs(timeout),
                out,
            )
            .await
        }
        Command::Log { host, apply_id } => log::log(client, host, apply_id, out).await,
        Command::Promote { yes } => promote::promote(client, yes, out).await,
        Command::Audit { limit } => audit::audit(client, limit, out).await,
    }
}
