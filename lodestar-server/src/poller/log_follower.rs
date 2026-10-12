//! Streams a running apply's log to dashboard clients.

use std::time::Duration;

use lodestar_proto::WsMessage;

use super::host_poll::log_err;
use super::reachability::agent_base;
use crate::config::ManagedHost;
use crate::db;
use crate::state::AppState;

const LOG_INTERVAL: Duration = Duration::from_secs(2);
/// A follower gives up after this many failed log reads in a row.
const LOG_MAX_ERRORS: u32 = 5;

/// Removes the follower entry when the follower ends, however it ends.
struct FollowerGuard<'a> {
    state: &'a AppState,
    key: (String, String),
}

impl Drop for FollowerGuard<'_> {
    fn drop(&mut self) {
        self.state.followers.lock().unwrap().remove(&self.key);
    }
}

/// Streams a running apply's log to dashboard clients every 2 s; at eof stores the full log.
/// At most one follower runs per `(host, apply_id)`.
pub async fn follow_log(state: &AppState, host: &ManagedHost, apply_id: &str) {
    let key = (host.name.clone(), apply_id.to_string());
    if !state.followers.lock().unwrap().insert(key.clone()) {
        return;
    }
    let _guard = FollowerGuard { state, key };
    let peers = state.online.peers().await;
    let Some(base) = agent_base(&state.config, host, &peers) else {
        return;
    };
    let mut log = String::new();
    // The agent's byte offset: `chunk.next`, not `log.len()` (replaced invalid UTF-8 changes
    // the length of the text).
    let mut offset = 0;
    let mut errors = 0;
    loop {
        match state.client.log(&base, apply_id, offset).await {
            Ok(chunk) => {
                errors = 0;
                log.push_str(&chunk.data);
                let eof = chunk.eof;
                let progressed = chunk.next > offset;
                offset = offset.max(chunk.next);
                if progressed || eof {
                    state.broadcast(WsMessage::ApplyLog(chunk));
                }
                if eof {
                    if db::get_apply(&state.db, apply_id)
                        .await
                        .ok()
                        .flatten()
                        .is_some()
                    {
                        log_err(
                            "storing apply log",
                            &host.name,
                            db::set_apply_log(&state.db, apply_id, &log).await,
                        );
                    }
                    return;
                }
            }
            Err(error) => {
                errors += 1;
                if errors >= LOG_MAX_ERRORS {
                    eprintln!(
                        "lodestar-server: {}: giving up on log of {apply_id}: {error:#}",
                        host.name
                    );
                    return;
                }
            }
        }
        tokio::time::sleep(LOG_INTERVAL).await;
    }
}
