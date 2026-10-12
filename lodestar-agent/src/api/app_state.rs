use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use crate::apply::read_record;
use crate::config::AgentConfig;
use crate::git::Git;
use crate::net::SourceGuard;

const PENDING_TTL: Duration = Duration::from_secs(30);

/// An apply launched but not yet visible as a record: its id and launch time.
pub(crate) type Pending = (String, Instant);

#[derive(Clone)]
pub struct AppState {
    pub(crate) config: Arc<AgentConfig>,
    pub(crate) config_path: Arc<PathBuf>,
    pub(crate) git: Arc<Git>,
    pub(crate) guard: Arc<SourceGuard>,
    /// Serialises the "is anything running?" check with the launch (and the
    /// background fetch). Holds the apply launched but not yet visible as a
    /// record, for up to `PENDING_TTL`.
    pub(crate) apply_lock: Arc<Mutex<Option<Pending>>>,
}

impl AppState {
    /// `config_path` is handed to the apply wrapper, which re-reads it.
    pub fn new(
        config: AgentConfig,
        config_path: PathBuf,
        guard: SourceGuard,
    ) -> anyhow::Result<AppState> {
        Ok(AppState {
            git: Arc::new(Git::new(&config)?),
            config: Arc::new(config),
            config_path: Arc::new(config_path),
            guard: Arc::new(guard),
            apply_lock: Arc::new(Mutex::new(None)),
        })
    }
}

/// The pending apply, if its record has not appeared yet and it is recent.
pub(crate) fn live_pending(state: &AppState, pending: &mut Option<Pending>) -> Option<String> {
    if let Some((id, launched_at)) = pending.as_ref()
        && (launched_at.elapsed() >= PENDING_TTL || read_record(&state.config, id).is_some())
    {
        *pending = None;
    }
    pending.as_ref().map(|(id, _)| id.clone())
}
