//! Dashboard access control: only the owner's Tailscale identities get in.

mod actor;
mod decision;
mod middleware;

pub use actor::Actor;
pub use decision::{Access, decide};
pub use middleware::{AccessState, require_access};
