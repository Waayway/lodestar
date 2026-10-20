//! Server bootstrap shared by `lodestar-server serve` and `lodestar-server demo`: database, git
//! mirror, poller and the dashboard listener.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use tokio::task::JoinHandle;

use crate::config::{Link, ManagedHost, ServerConfig};
use crate::mirror::Mirror;
use crate::state::AppState;
use crate::tailscale::OnlineProvider;
use crate::{api, db, poller};

/// A running server. Dropping it stops the listener and the poller.
pub struct ServerHandle {
    /// The address actually bound (useful with port 0).
    pub addr: SocketAddr,
    pub state: AppState,
    serve: JoinHandle<std::io::Result<()>>,
    background: Vec<JoinHandle<()>>,
    /// Asks the listener to stop accepting and to close idle connections.
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl ServerHandle {
    /// Serves until the listener fails. Dropping this future (and the handle) still stops
    /// the server.
    pub async fn wait(mut self) -> anyhow::Result<()> {
        (&mut self.serve)
            .await
            .context("server task")?
            .context("serving")
    }
}

impl ServerHandle {
    /// Stops the server like a process exit would: no new connections, open connections
    /// closed (idle ones at once, others after their current request; waits up to 5 s),
    /// background tasks stopped. The address can be bound again afterwards.
    pub async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        for task in &self.background {
            task.abort();
        }
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), &mut self.serve).await;
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        self.serve.abort();
        for task in &self.background {
            task.abort();
        }
    }
}

/// Starts the server and returns once it is listening on `config.listen`.
pub async fn start(
    config: ServerConfig,
    hosts: Vec<ManagedHost>,
    links: Vec<Link>,
    online: Arc<dyn OnlineProvider>,
    demo: bool,
) -> anyhow::Result<ServerHandle> {
    config.check_listen()?;
    config.check_proxy()?;
    let connection = db::connect(&config.state_dir).await?;
    let mirror = Arc::new(Mirror::open_or_init(&config).await?);
    let listen = config.listen;
    let mut state = AppState::new(Arc::new(config), connection, mirror, online, hosts)?;
    state.links = Arc::new(links);
    state.demo = demo;
    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .with_context(|| format!("binding {listen}"))?;
    let addr = listener.local_addr().context("listener address")?;
    let background = poller::spawn(state.clone());
    let app = api::app(state.clone()).into_make_service_with_connect_info::<SocketAddr>();
    let (shutdown, stopped) = tokio::sync::oneshot::channel::<()>();
    let serve = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = stopped.await;
            })
            .await
    });
    eprintln!("lodestar-server: listening on {addr}");
    Ok(ServerHandle {
        addr,
        state,
        serve,
        background,
        shutdown: Some(shutdown),
    })
}
