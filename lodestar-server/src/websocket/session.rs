//! One dashboard WebSocket: snapshot first, then broadcasts; lagged -> fresh snapshot. A
//! socket opened with a login session closes once that session is gone.

use axum::extract::ws::{Message, WebSocket};
use futures::SinkExt;
use lodestar_proto::WsMessage;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::{Interval, MissedTickBehavior};

use crate::access::Viewer;
use crate::state::{AppState, snapshot};
use crate::{clock, db};

async fn send(socket: &mut WebSocket, message: &WsMessage) -> bool {
    let Ok(text) = serde_json::to_string(message) else {
        return false;
    };
    socket.send(Message::Text(text.into())).await.is_ok()
}

async fn send_snapshot(socket: &mut WebSocket, state: &AppState, viewer: &str) -> bool {
    let snapshot = snapshot(state, viewer.to_string()).await;
    send(socket, &WsMessage::Snapshot(snapshot)).await
}

/// Ticks every `session_recheck` for a viewer with a login session; `None` otherwise (the
/// tailnet listener has no sessions).
fn session_recheck(state: &AppState, viewer: &Viewer) -> Option<Interval> {
    let auth = state.auth.as_ref()?;
    viewer.session_hash.as_ref()?;
    let mut interval = tokio::time::interval_at(
        tokio::time::Instant::now() + auth.session_recheck,
        auth.session_recheck,
    );
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    Some(interval)
}

/// Whether the viewer's login session still exists (a viewer without one always passes). A
/// database error keeps the socket open: the next check decides.
async fn session_alive(state: &AppState, viewer: &Viewer) -> bool {
    let Some(hash) = &viewer.session_hash else {
        return true;
    };
    match db::find_session(&state.db, hash, clock::now()).await {
        Ok(session) => session.is_some(),
        Err(error) => {
            eprintln!("lodestar-server: rechecking a WebSocket session: {error:#}");
            true
        }
    }
}

/// Waits for the next recheck; never completes without one.
async fn next_recheck(recheck: &mut Option<Interval>) {
    match recheck {
        Some(interval) => {
            interval.tick().await;
        }
        None => std::future::pending().await,
    }
}

pub(super) async fn run(state: AppState, viewer: Viewer, mut socket: WebSocket) {
    let name = viewer.name.clone().unwrap_or_default();
    let mut recheck = session_recheck(&state, &viewer);
    // Subscribe before building the snapshot so no update in between is lost.
    let mut receiver = state.tx.subscribe();
    if !send_snapshot(&mut socket, &state, &name).await {
        return;
    }
    loop {
        tokio::select! {
            received = receiver.recv() => {
                let ok = match received {
                    Ok(message) => send(&mut socket, &message).await,
                    Err(RecvError::Lagged(_)) => send_snapshot(&mut socket, &state, &name).await,
                    Err(RecvError::Closed) => false,
                };
                if !ok {
                    break;
                }
            }
            incoming = socket.recv() => match incoming {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                Some(Ok(_)) => {}
            },
            () = next_recheck(&mut recheck) => {
                if !session_alive(&state, &viewer).await {
                    break;
                }
            }
        }
    }
    // Completes the close handshake (or closes from our side); errors mean it's gone already.
    let _ = socket.close().await;
}
