//! Streams a running apply's log to dashboard clients.

use std::time::Duration;

use lodestar_proto::WsMessage;

use super::host_poll::log_err;
use super::log_cap::{LogCap, MAX_LOG_BYTES, Next};
use super::reachability::agent_base;
use crate::config::ManagedHost;
use crate::db;
use crate::state::AppState;

const LOG_INTERVAL: Duration = Duration::from_secs(2);
/// A follower gives up after this many failed (or refused) log reads in a row.
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

/// Streams a running apply's log to dashboard clients every 2 s (at once while a full chunk
/// says more is waiting); at eof stores the full log. A log over `MAX_LOG_BYTES` is stored cut
/// there and not followed again until the apply ends. At most one follower runs per
/// `(host, apply_id)`.
pub async fn follow_log(state: &AppState, host: &ManagedHost, apply_id: &str) {
    let key = (host.name.clone(), apply_id.to_string());
    if state.capped_logs.lock().unwrap().contains(&key)
        || !state.followers.lock().unwrap().insert(key.clone())
    {
        return;
    }
    let _guard = FollowerGuard {
        state,
        key: key.clone(),
    };
    let peers = state.online.peers().await;
    let Some(base) = agent_base(&state.config, host, &peers) else {
        return;
    };
    let mut cap = LogCap::new();
    let mut errors = 0;
    loop {
        let offset = cap.offset();
        let result = state
            .client
            .log(&base, apply_id, offset)
            .await
            .and_then(|chunk| cap.accept(&chunk).map(|next| (chunk, next)));
        match result {
            Ok((chunk, next)) => {
                errors = 0;
                if chunk.next > offset || chunk.eof {
                    state.broadcast(WsMessage::ApplyLog(chunk));
                }
                if next == Next::Done {
                    if cap.truncated() {
                        eprintln!(
                            "lodestar-server: {}: log of {apply_id} is over {MAX_LOG_BYTES} \
                             bytes; keeping the start only",
                            host.name
                        );
                        state.capped_logs.lock().unwrap().insert(key);
                    }
                    store_log(state, host, apply_id, cap.log()).await;
                    return;
                }
                if next == Next::More {
                    continue;
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
        cap.reset_reads();
        tokio::time::sleep(LOG_INTERVAL).await;
    }
}

async fn store_log(state: &AppState, host: &ManagedHost, apply_id: &str, log: &str) {
    if db::get_apply(&state.db, apply_id)
        .await
        .ok()
        .flatten()
        .is_some()
    {
        log_err(
            "storing apply log",
            &host.name,
            db::set_apply_log(&state.db, apply_id, log).await,
        );
    }
}
