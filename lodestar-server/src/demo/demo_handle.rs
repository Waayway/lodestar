use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tempfile::TempDir;

use super::tasks::Tasks;
use crate::config::{ManagedHost, ServerConfig};
use crate::server::{self, ServerHandle};
use crate::state::AppState;
use crate::tailscale::OnlineProvider;

/// A running demo. Dropping it stops the server, the simulated agents and the commit
/// ticker, then deletes the temp dir.
pub struct DemoHandle {
    pub addr: SocketAddr,
    pub state: AppState,
    /// `None` only while `restart` has it stopped.
    pub(super) server: Option<ServerHandle>,
    /// What `restart` needs to start the server again on the same state.
    pub(super) config: ServerConfig,
    pub(super) hosts: Vec<ManagedHost>,
    pub(super) online: Arc<dyn OnlineProvider>,
    pub(super) tasks: Tasks,
    pub(super) _tmp: TempDir,
}

impl DemoHandle {
    /// Serves until the server fails.
    pub async fn wait(self) -> anyhow::Result<()> {
        let DemoHandle {
            server,
            tasks,
            _tmp,
            ..
        } = self;
        let result = match server {
            Some(server) => server.wait().await,
            None => Err(anyhow::anyhow!("the demo server is stopped")),
        };
        drop(tasks);
        result
    }

    /// Simulates a restart of the server process (e.g. by a switch of the host it runs on):
    /// stops the server, leaves the address dead for `down`, then starts it again on the
    /// same address, database and mirror. The simulated agents keep running.
    pub async fn restart(&mut self, down: Duration) -> anyhow::Result<()> {
        self.stop_server().await;
        tokio::time::sleep(down).await;
        self.start_server().await
    }

    /// Stops the server (see `restart`); the address is free until `start_server`.
    pub async fn stop_server(&mut self) {
        if let Some(old) = self.server.take() {
            old.stop().await;
        }
    }

    /// Starts the server again after `stop_server`, on the same address and state.
    pub async fn start_server(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(self.server.is_none(), "the demo server is already running");
        let mut config = self.config.clone();
        config.listen = self.addr;
        let new = server::start(config, self.hosts.clone(), self.online.clone(), true).await?;
        self.state = new.state.clone();
        self.server = Some(new);
        Ok(())
    }
}
