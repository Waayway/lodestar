//! Dashboard access control: on the tailnet listener only requests naming this server
//! (`Host`) from the owner's Tailscale identities get in; on the router entrance only trusted
//! proxies, with a viewer from the session. On both, a browser's `Origin` must be our own.

mod actor;
mod authority;
mod decision;
mod host_allowlist;
mod middleware;
mod origin_policy;
mod proxy_middleware;
mod viewer;

pub use actor::Actor;
pub use authority::{Authority, AuthorityHost, Scheme, single_header};
pub use decision::{Access, decide};
pub use host_allowlist::{HostAllowlist, parse_entry, request_authority, require_known_host};
pub use middleware::{AccessState, require_access, require_need};
pub use origin_policy::{OriginPolicy, require_origin};
pub use proxy_middleware::{
    TAILNET_MARK_HEADER, TAILNET_MARK_VALUE, load_viewer, require_public_origin,
    require_trusted_peer,
};
pub use viewer::{Need, Viewer};
