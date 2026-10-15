//! Demo services: a tiny in-process HTTP server and links that cover every state the
//! Services tab can show.

use std::time::Duration;

use anyhow::Context;
use axum::Router;
use axum::http::StatusCode;
use axum::routing::get;
use tokio::task::JoinHandle;

use crate::config::{Link, LinkCheck};

const SLOW_SECS: f64 = 1.2;
/// The demo host that is simulated as offline at start.
const OFFLINE_HOST: &str = "laptop-2";
const EXPECT: [u16; 9] = [200, 204, 301, 302, 303, 307, 308, 401, 403];

#[allow(clippy::too_many_arguments)]
fn link(
    id: &str,
    title: &str,
    url: String,
    group: &str,
    description: &str,
    icon: Option<&str>,
    host: &str,
    enable: bool,
) -> Link {
    Link {
        id: id.into(),
        title: title.into(),
        url,
        group: group.into(),
        description: Some(description.into()),
        icon: icon.map(Into::into),
        host: host.into(),
        check: LinkCheck {
            enable,
            url: None,
            expect_status: EXPECT.to_vec(),
            insecure_tls: false,
        },
    }
}

/// Starts the server on a free loopback port and returns the demo links with its task.
pub(crate) async fn start_demo_services() -> anyhow::Result<(Vec<Link>, JoinHandle<()>)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .context("binding the demo services server")?;
    let addr = listener.local_addr()?;
    let router = Router::new()
        .route("/", get(|| async { "ok" }))
        .route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_secs_f64(SLOW_SECS)).await;
                "ok"
            }),
        )
        .route("/bad", get(|| async { StatusCode::BAD_GATEWAY }));
    let task = tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, router).await {
            eprintln!("lodestar-server: demo services: {error}");
        }
    });
    // A port nothing listens on.
    let closed = std::net::TcpListener::bind("127.0.0.1:0")
        .context("finding a free port")?
        .local_addr()?;
    let base = format!("http://{addr}");
    let links = vec![
        link(
            "nextcloud",
            "Nextcloud",
            format!("{base}/"),
            "Apps",
            "Files, calendar and contacts",
            Some("nextcloud"),
            "server-1",
            true,
        ),
        link(
            "immich",
            "Immich",
            format!("{base}/slow"),
            "Apps",
            "Photo library (slow to answer)",
            Some("immich"),
            "server-1",
            true,
        ),
        link(
            "jellyfin",
            "Jellyfin",
            format!("{base}/bad"),
            "Media",
            "Streaming server returning 502",
            Some("jellyfin"),
            "media",
            true,
        ),
        link(
            "traefik",
            "Traefik",
            format!("http://{closed}/"),
            "Infrastructure",
            "Reverse proxy, connection refused",
            Some("traefikproxy"),
            "server-1",
            true,
        ),
        link(
            "proxmox",
            "Proxmox",
            "https://pve.example.test:8006/".into(),
            "Infrastructure",
            "Hypervisor; not checked",
            Some("proxmox"),
            "server-1",
            false,
        ),
        link(
            "truenas",
            "TrueNAS",
            format!("{base}/"),
            "Infrastructure",
            "Storage on a host that is offline",
            Some("truenas"),
            OFFLINE_HOST,
            true,
        ),
    ];
    Ok((links, task))
}
