use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use lodestar_proto::{ServiceCheck, WsMessage};
use sea_orm::DatabaseConnection;
use tokio::sync::broadcast;

use crate::agent_client::AgentClient;
use crate::auth::AuthState;
use crate::builds::{BuildBackoff, BuildGate, Builder, PrebuildQueue, RemoteBuilder};
use crate::config::{Link, ManagedHost, ServerConfig};
use crate::mirror::Mirror;
use crate::tailscale::OnlineProvider;
use crate::updates::nix_runner::{NixCommand, NixRunner};
use crate::updates::updates_state::UpdatesState;
use crate::updates::upstream::{GithubProvider, LatestVersion, UpstreamProvider};

const BROADCAST_CAPACITY: usize = 256;

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub mirror: Arc<Mirror>,
    pub online: Arc<dyn OnlineProvider>,
    pub client: AgentClient,
    pub hosts: Vec<ManagedHost>,
    /// Service links declared in Nix.
    pub links: Arc<Vec<Link>>,
    /// Latest check result per link id.
    pub checks: Arc<Mutex<HashMap<String, ServiceCheck>>>,
    /// Sender of the dashboard broadcast (WebSocket clients subscribe to it).
    pub tx: broadcast::Sender<WsMessage>,
    pub config: Arc<ServerConfig>,
    /// Hosts whose last `GET /status` succeeded (by host name).
    pub(crate) reachable: Arc<Mutex<HashMap<String, bool>>>,
    /// Whether each host's agent answered the last time it was contacted (offline hosts are
    /// not contacted), so the poller logs only changes.
    pub(crate) agent_up: Arc<Mutex<HashMap<String, bool>>>,
    /// Latest tracked software versions per host name; replaced on every status, never merged.
    pub versions: Arc<Mutex<HashMap<String, BTreeMap<String, String>>>>,
    /// Latest upstream release per software name.
    pub latest: Arc<Mutex<HashMap<String, LatestVersion>>>,
    /// Where upstream releases come from (tests inject a fake).
    pub upstream: Arc<dyn UpstreamProvider>,
    /// The updater's lock and last run per input.
    pub updates: Arc<UpdatesState>,
    /// How the updater runs Nix (tests inject a fake).
    pub nix: Arc<dyn NixRunner>,
    /// Builds and publishes on the builder; never called while `builds.enable` is off (tests
    /// inject a fake).
    pub builder: Arc<dyn Builder>,
    /// One build per commit: the lock and result files under `state_dir/builds`.
    pub builds: Arc<BuildGate>,
    /// The commit the gate is building for each channel right now (prebuild, apply or re-send).
    pub building: Arc<Mutex<BTreeMap<String, String>>>,
    /// How long the last build the gate ran took, per channel (seconds); for the metrics.
    pub build_durations: Arc<Mutex<BTreeMap<String, f64>>>,
    /// When the prebuild and the re-send may try again a build that could not run (tests set
    /// shorter pauses).
    pub build_backoff: Arc<BuildBackoff>,
    /// Prebuilds of new channel heads: one at a time, the last-seen heads.
    pub(crate) prebuilds: Arc<PrebuildQueue>,
    /// `(host, apply_id)` re-sends waiting for their commit's build.
    pub(crate) resend_builds: Arc<Mutex<HashSet<(String, String)>>>,
    /// `(host, apply_id)` re-sends refused for a failed build (logged once).
    pub(crate) resend_refused: Arc<Mutex<HashSet<(String, String)>>>,
    /// `(host, apply_id)` pairs with a live log follower.
    pub(crate) followers: Arc<Mutex<HashSet<(String, String)>>>,
    /// Serialises dashboard actions (apply, promote) from mirror fetch to recorded result, so
    /// the last press always records the newest channel head.
    pub(crate) actions: Arc<tokio::sync::Mutex<()>>,
    /// Demo mode (simulated hosts); reported in the snapshot.
    pub demo: bool,
    /// The router entrance's login state; `None` when there is no router entrance.
    pub auth: Option<Arc<AuthState>>,
}

impl AppState {
    pub fn new(
        config: Arc<ServerConfig>,
        db: DatabaseConnection,
        mirror: Arc<Mirror>,
        online: Arc<dyn OnlineProvider>,
        hosts: Vec<ManagedHost>,
    ) -> anyhow::Result<Self> {
        let (sender, _) = broadcast::channel(BROADCAST_CAPACITY);
        let nix = Arc::new(NixCommand {
            bin: config.nix_bin.clone(),
        });
        let builder = Arc::new(RemoteBuilder::from_config(&config));
        let builds = Arc::new(BuildGate::new(config.state_dir.join("builds")));
        Ok(Self {
            db,
            mirror,
            online,
            client: AgentClient::new()?,
            hosts,
            links: Arc::default(),
            checks: Arc::default(),
            tx: sender,
            config,
            reachable: Arc::default(),
            agent_up: Arc::default(),
            versions: Arc::default(),
            latest: Arc::default(),
            upstream: Arc::new(GithubProvider::new()?),
            updates: Arc::default(),
            nix,
            builder,
            builds,
            building: Arc::default(),
            build_durations: Arc::default(),
            build_backoff: Arc::default(),
            prebuilds: Arc::default(),
            resend_builds: Arc::default(),
            resend_refused: Arc::default(),
            followers: Arc::default(),
            actions: Arc::default(),
            demo: false,
            auth: None,
        })
    }

    /// Sends to dashboard clients; having none is fine.
    pub fn broadcast(&self, message: WsMessage) {
        let _ = self.tx.send(message);
    }
}
