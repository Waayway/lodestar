//! `GET /api/ws`: a full `Snapshot` first, then every broadcast `WsMessage`. A client that
//! falls behind the broadcast buffer gets a fresh `Snapshot` instead of the missed messages.
//! Messages from the client are ignored; the socket only carries server -> browser updates.

mod handler;
mod session;

pub use handler::handler;
