//! The dashboard HTTP surface: JSON API under `/api`, the WebSocket at `/api/ws`, and the
//! embedded frontend for everything else. On the tailnet listener every route, the WebSocket
//! upgrade and the static files included, sits behind `require_access`; on the router entrance
//! behind `require_trusted_peer` and `load_viewer`. Route groups need a viewer that permits
//! them; state-changing requests also need `require_same_site` (CSRF).

mod app;
mod apply_request;
mod audit;
mod channels;
mod csrf;
mod error;
mod hosts;
mod me;
mod promote;
mod public_services;
mod snapshot;

pub use app::{app, proxy_app};
pub use csrf::LODESTAR_HEADER;
