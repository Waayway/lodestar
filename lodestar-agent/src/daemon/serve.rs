use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use axum::Router;

use super::background_fetch::background_fetch;
use super::manager_addresses::log_manager_addrs;
use crate::api::{AppState, router};
use crate::apply::clear_stale_marker;
use crate::config::AgentConfig;
use crate::net::{SourceGuard, tailscale_addrs};

const ADDR_RECHECK: Duration = Duration::from_secs(60);

async fn serve_on(listener: tokio::net::TcpListener, app: Router) {
    let app = app.into_make_service_with_connect_info::<SocketAddr>();
    if let Err(error) = axum::serve(listener, app).await {
        eprintln!("lodestar-agent: server stopped: {error}");
    }
}

/// Runs the daemon: binds `listen`, or every Tailscale address (retrying with
/// backoff and re-checking the addresses every 60 s), and serves forever.
pub async fn serve(config: AgentConfig, config_path: PathBuf) -> anyhow::Result<()> {
    clear_stale_marker(&config);
    log_manager_addrs(&config.manager_host).await;
    let state = AppState::new(
        config.clone(),
        config_path,
        SourceGuard::new(config.manager_host.clone()),
    )?;
    tokio::spawn(background_fetch(state.clone()));
    let app = router(state);

    if let Some(address) = config.listen {
        let listener = tokio::net::TcpListener::bind(address).await?;
        eprintln!("lodestar-agent: listening on {address}");
        serve_on(listener, app).await;
        anyhow::bail!("server exited");
    }

    let mut bound: HashSet<IpAddr> = HashSet::new();
    let mut backoff = Duration::from_secs(1);
    loop {
        let addresses = tailscale_addrs();
        let mut pending = addresses.is_empty();
        if pending {
            eprintln!("lodestar-agent: no Tailscale address yet; retrying");
        }
        let before = bound.len();
        for ip in addresses {
            if bound.contains(&ip) {
                continue;
            }
            match tokio::net::TcpListener::bind(SocketAddr::new(ip, config.port)).await {
                Ok(listener) => {
                    eprintln!(
                        "lodestar-agent: listening on {}",
                        SocketAddr::new(ip, config.port)
                    );
                    bound.insert(ip);
                    tokio::spawn(serve_on(listener, app.clone()));
                }
                Err(error) => {
                    eprintln!("lodestar-agent: cannot bind {ip}:{}: {error}", config.port);
                    pending = true;
                }
            }
        }
        if bound.len() != before {
            let mut all: Vec<String> = bound
                .iter()
                .map(|ip| SocketAddr::new(*ip, config.port).to_string())
                .collect();
            all.sort();
            eprintln!("lodestar-agent: bound addresses: {}", all.join(", "));
        }
        if pending {
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(ADDR_RECHECK);
        } else {
            backoff = Duration::from_secs(1);
            tokio::time::sleep(ADDR_RECHECK).await;
        }
    }
}
