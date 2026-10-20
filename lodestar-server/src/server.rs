//! Server bootstrap shared by `lodestar-server serve` and `lodestar-server demo`: database, git
//! mirror, poller, the dashboard listener and, when configured, the router entrance.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::auth::AuthState;
use crate::config::{Link, ManagedHost, ServerConfig};
use crate::mirror::Mirror;
use crate::state::AppState;
use crate::tailscale::OnlineProvider;
use crate::{api, db, poller};

/// A running server. Dropping it stops the listeners and the poller.
pub struct ServerHandle {
    /// The address actually bound (useful with port 0).
    pub addr: SocketAddr,
    /// The router entrance's address, when one is configured.
    pub proxy_addr: Option<SocketAddr>,
    pub state: AppState,
    /// One serve task per listener.
    serves: Vec<JoinHandle<std::io::Result<()>>>,
    background: Vec<JoinHandle<()>>,
    /// Asks every listener to stop accepting and to close idle connections (also when
    /// dropped).
    shutdown: Option<watch::Sender<()>>,
}

impl ServerHandle {
    /// Serves until a listener fails. Dropping this future (and the handle) still stops
    /// the server.
    pub async fn wait(mut self) -> anyhow::Result<()> {
        let (result, _, _) = futures::future::select_all(self.serves.iter_mut()).await;
        result.context("server task")?.context("serving")
    }
}

impl ServerHandle {
    /// Stops the server like a process exit would: no new connections, open connections
    /// closed (idle ones at once, others after their current request; waits up to 5 s),
    /// background tasks stopped. The addresses can be bound again afterwards.
    pub async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        for task in &self.background {
            task.abort();
        }
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            futures::future::join_all(self.serves.iter_mut()),
        )
        .await;
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        for task in &self.serves {
            task.abort();
        }
        for task in &self.background {
            task.abort();
        }
    }
}

/// Serves `app` on `listener` until `shutdown` fires or its sender is dropped.
fn spawn_serve(
    listener: TcpListener,
    app: axum::Router,
    mut shutdown: watch::Receiver<()>,
) -> JoinHandle<std::io::Result<()>> {
    let app = app.into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown.changed().await;
            })
            .await
    })
}

async fn bind(address: SocketAddr) -> anyhow::Result<(TcpListener, SocketAddr)> {
    let listener = TcpListener::bind(address)
        .await
        .with_context(|| format!("binding {address}"))?;
    let bound = listener.local_addr().context("listener address")?;
    Ok((listener, bound))
}

/// Starts the server and returns once it is listening on `config.listen` (and on
/// `config.proxy.listen` when set).
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
    let proxy = config.proxy.clone();
    let mut state = AppState::new(Arc::new(config), connection, mirror, online, hosts)?;
    state.links = Arc::new(links);
    state.demo = demo;
    if let Some(proxy) = &proxy {
        state.auth = Some(Arc::new(AuthState::new(proxy.clone())?));
    }
    let (listener, addr) = bind(listen).await?;
    let proxy_listener = match &proxy {
        Some(proxy) => Some(bind(proxy.listen).await?),
        None => None,
    };
    let (shutdown, stopped) = watch::channel(());
    let background = poller::spawn(state.clone());
    let mut serves = vec![spawn_serve(
        listener,
        api::app(state.clone()),
        stopped.clone(),
    )];
    eprintln!("lodestar-server: listening on {addr}");
    let proxy_addr = proxy_listener.map(|(listener, proxy_addr)| {
        serves.push(spawn_serve(
            listener,
            api::proxy_app(state.clone()),
            stopped,
        ));
        eprintln!("lodestar-server: router entrance on {proxy_addr}");
        proxy_addr
    });
    Ok(ServerHandle {
        addr,
        proxy_addr,
        state,
        serves,
        background,
        shutdown: Some(shutdown),
    })
}
