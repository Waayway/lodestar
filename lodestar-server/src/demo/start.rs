use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;

use super::DemoHandle;
use super::agents::{self, HostSetup, SimAgent};
use super::demo_host::DEMO_HOSTS;
use super::demo_online::DemoOnline;
use super::demo_peer::DemoPeer;
use super::repo::{DemoRepo, STABLE_AT};
use super::tasks::Tasks;
use crate::clock::now;
use crate::config::{AccessConfig, Link, ManagedHost, OnlineSource, ServerConfig};
use crate::server;
use crate::tailscale::OnlineProvider;

/// Seconds between new commits on `unstable`.
const COMMIT_EVERY_SECS: f64 = 180.0;
const POLL_SECS: f64 = 5.0;
const MIRROR_FETCH_SECS: f64 = 30.0;
const DEMO_USER: &str = "demo";

fn scaled(secs: f64, speed: f64) -> Duration {
    Duration::from_secs_f64(secs / speed)
}

fn scaled_secs(secs: f64, speed: f64) -> u64 {
    ((secs / speed).round() as u64).max(1)
}

/// Starts the demo; returns once the dashboard is listening on `listen`.
pub async fn start(listen: SocketAddr, speed: f64) -> anyhow::Result<DemoHandle> {
    anyhow::ensure!(
        speed.is_finite() && speed > 0.0,
        "speed must be a positive number"
    );
    if !listen.ip().is_loopback() {
        eprintln!(
            "lodestar-server: warning: demo mode has no access control, and {listen} is not a loopback address"
        );
    }
    let temporary_dir = tempfile::Builder::new()
        .prefix("lodestar-demo-")
        .tempdir()
        .context("creating the demo temp dir")?;
    let (mut repo, commits) = DemoRepo::create(temporary_dir.path()).await?;

    let mut tasks = Vec::new();
    let mut hosts = Vec::new();
    let mut peers = Vec::new();
    let mut addresses = HashMap::new();
    let start = now();
    // The port for agents that don't answer. Held until every simulated agent is bound, so
    // none of them can get it; dropped afterwards, so nothing listens on it.
    let closed = std::net::TcpListener::bind("127.0.0.1:0").context("finding a free port")?;
    let closed_addr = closed.local_addr()?;
    for (index, demo_host) in DEMO_HOSTS.iter().enumerate() {
        let os = if demo_host.system.ends_with("darwin") {
            "darwin"
        } else {
            "linux"
        };
        let head = if demo_host.channel == "stable" {
            STABLE_AT
        } else {
            commits.len()
        };
        let running = commits[head - 1 - demo_host.behind].clone();
        let addr = if demo_host.unreachable {
            closed_addr
        } else {
            let setup = HostSetup {
                name: demo_host.name.into(),
                os: os.into(),
                channel: demo_host.channel.into(),
                user: DEMO_USER.into(),
                running,
                script: demo_host.script,
                generation: 40 + 7 * index as u64,
                activated_at: start - (2 + index as i64) * 3600,
            };
            let (addr, task) = agents::serve(Arc::new(SimAgent::new(setup, speed))).await?;
            tasks.push(task);
            addr
        };
        addresses.insert(demo_host.name.to_string(), addr.to_string());
        peers.push(DemoPeer {
            host: demo_host.name.into(),
            os: if os == "darwin" { "macOS" } else { "linux" }.into(),
            addr: addr.ip(),
            offline_for: demo_host.offline_for.map(|seconds| scaled(seconds, speed)),
        });
        hosts.push(ManagedHost {
            name: demo_host.name.into(),
            system: demo_host.system.into(),
            user: DEMO_USER.into(),
            channel: demo_host.channel.into(),
            os: os.into(),
        });
    }
    drop(closed);

    let hosts_file = temporary_dir.path().join("hosts.json");
    std::fs::write(&hosts_file, serde_json::to_vec_pretty(&hosts)?)
        .with_context(|| format!("writing {}", hosts_file.display()))?;
    let repo_url = repo
        .bare
        .to_str()
        .context("temp dir path is not UTF-8")?
        .to_string();
    let config = ServerConfig {
        listen,
        agent_port: 7461,
        hosts_file,
        links_file: None,
        repo_url: repo_url.clone(),
        push_url: repo_url,
        deploy_key_file: None,
        state_dir: temporary_dir.path().join("state"),
        access: AccessConfig::Disabled,
        // Agent addresses with their ports; reachability itself comes from `DemoOnline`.
        online: OnlineSource::Static { addresses },
        mirror_fetch_secs: scaled_secs(MIRROR_FETCH_SECS, speed),
        poll_secs: scaled_secs(POLL_SECS, speed),
        tailscale_bin: "tailscale".into(),
        promote: Some(lodestar_proto::PromoteChannels::default()),
    };
    let links: Vec<Link> = Vec::new();
    let online: Arc<dyn OnlineProvider> = Arc::new(DemoOnline::new(peers));
    let server = server::start(
        config.clone(),
        hosts.clone(),
        links.clone(),
        online.clone(),
        true,
    )
    .await?;

    let every = scaled(COMMIT_EVERY_SECS, speed);
    tasks.push(tokio::spawn(async move {
        let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + every, every);
        loop {
            tick.tick().await;
            match repo.land_commit().await {
                Ok(commit) => eprintln!("lodestar-server: demo: new commit {commit} on unstable"),
                Err(error) => eprintln!("lodestar-server: demo: new commit: {error:#}"),
            }
        }
    }));

    Ok(DemoHandle {
        addr: server.addr,
        state: server.state.clone(),
        server: Some(server),
        config,
        hosts,
        links,
        online,
        tasks: Tasks(tasks),
        _tmp: temporary_dir,
    })
}
