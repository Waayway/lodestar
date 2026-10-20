//! Shared server state and the dashboard views built from it.

mod app_state;
mod host_view;
mod public_services;
mod service_view;
mod snapshot;

pub use app_state::AppState;
pub use host_view::host_view;
pub(crate) use host_view::{host_view_with, online_state};
pub use public_services::public_services;
pub use service_view::service_views;
pub use snapshot::{channels, snapshot};
