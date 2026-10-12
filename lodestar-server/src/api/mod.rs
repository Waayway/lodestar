//! The dashboard HTTP surface: JSON API under `/api`, the WebSocket at `/api/ws`, and the
//! embedded frontend for everything else. Every route, the WebSocket upgrade and the static
//! files included, sits behind `require_access`; state-changing requests also need
//! `require_same_site` (CSRF).

mod app;
mod apply_request;
mod audit;
mod channels;
mod csrf;
mod error;
mod hosts;
mod promote;
mod snapshot;

pub use app::app;
pub use csrf::LODESTAR_HEADER;
