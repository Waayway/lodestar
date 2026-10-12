use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use lodestar_proto::WsMessage;
use sea_orm::DatabaseConnection;
use tokio::sync::broadcast;

use crate::agent_client::AgentClient;
use crate::config::{ManagedHost, ServerConfig};
use crate::mirror::Mirror;
use crate::tailscale::OnlineProvider;

const BROADCAST_CAPACITY: usize = 256;

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub mirror: Arc<Mirror>,
    pub online: Arc<dyn OnlineProvider>,
    pub client: AgentClient,
    pub hosts: Vec<ManagedHost>,
    /// Sender of the dashboard broadcast (WebSocket clients subscribe to it).
    pub tx: broadcast::Sender<WsMessage>,
    pub config: Arc<ServerConfig>,
    /// Hosts whose last `GET /status` succeeded (by host name).
    pub(crate) reachable: Arc<Mutex<HashMap<String, bool>>>,
    /// Whether each host's agent answered the last time it was contacted (offline hosts are
    /// not contacted), so the poller logs only changes.
    pub(crate) agent_up: Arc<Mutex<HashMap<String, bool>>>,
    /// `(host, apply_id)` pairs with a live log follower.
    pub(crate) followers: Arc<Mutex<HashSet<(String, String)>>>,
    /// Serialises dashboard actions (apply, promote) from mirror fetch to recorded result, so
    /// the last press always records the newest channel head.
    pub(crate) actions: Arc<tokio::sync::Mutex<()>>,
    /// Demo mode (simulated hosts); reported in the snapshot.
    pub demo: bool,
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
        Ok(Self {
            db,
            mirror,
            online,
            client: AgentClient::new()?,
            hosts,
            tx: sender,
            config,
            reachable: Arc::default(),
            agent_up: Arc::default(),
            followers: Arc::default(),
            actions: Arc::default(),
            demo: false,
        })
    }

    /// Sends to dashboard clients; having none is fine.
    pub fn broadcast(&self, message: WsMessage) {
        let _ = self.tx.send(message);
    }
}
