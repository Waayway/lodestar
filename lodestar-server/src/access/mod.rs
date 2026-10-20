//! Dashboard access control: on the tailnet listener only the owner's Tailscale identities
//! get in; on the router entrance only trusted proxies, with a viewer from the session.

mod actor;
mod decision;
mod middleware;
mod proxy_middleware;
mod viewer;

pub use actor::Actor;
pub use decision::{Access, decide};
pub use middleware::{AccessState, require_access, require_need};
pub use proxy_middleware::{
    TAILNET_MARK_HEADER, TAILNET_MARK_VALUE, load_viewer, require_public_origin,
    require_trusted_peer,
};
pub use viewer::{Need, Viewer};
