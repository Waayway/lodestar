//! Login: the OpenID Connect client, pending logins, sessions, their cookies and the
//! post-login redirect check.

pub mod cookie;
mod groups_claim;
mod login_error;
mod login_identity;
mod login_session;
mod login_start;
mod next_path;
mod oidc_client;
mod pending_login;
mod pending_logins;
mod session_id;

pub use groups_claim::GroupsClaim;
pub use login_error::LoginError;
pub use login_identity::LoginIdentity;
pub use login_session::LoginSession;
pub use login_start::LoginStart;
pub use next_path::safe_next;
pub use oidc_client::{DISCOVERY_TTL, OidcClient};
pub use pending_login::PendingLogin;
pub use pending_logins::{MAX_PENDING_LOGINS, PendingLogins};
pub use session_id::{SessionId, hash_session_value};
