//! One dashboard WebSocket: snapshot first, then broadcasts; lagged -> fresh snapshot.

use axum::extract::ws::{Message, WebSocket};
use futures::SinkExt;
use lodestar_proto::WsMessage;
use tokio::sync::broadcast::error::RecvError;

use crate::state::{AppState, snapshot};

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

pub(super) async fn run(state: AppState, viewer: String, mut socket: WebSocket) {
    // Subscribe before building the snapshot so no update in between is lost.
    let mut receiver = state.tx.subscribe();
    if !send_snapshot(&mut socket, &state, &viewer).await {
        return;
    }
    loop {
        tokio::select! {
            received = receiver.recv() => {
                let ok = match received {
                    Ok(message) => send(&mut socket, &message).await,
                    Err(RecvError::Lagged(_)) => send_snapshot(&mut socket, &state, &viewer).await,
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
        }
    }
    // Completes the close handshake (or closes from our side); errors mean it's gone already.
    let _ = socket.close().await;
}
