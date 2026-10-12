//! Tailscale address discovery and the manager source check.

mod source_guard;
mod tailscale_addresses;

pub use source_guard::{SourceGuard, manager_addrs};
pub use tailscale_addresses::{is_tailscale, tailscale_addrs};
