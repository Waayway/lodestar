//! Server configuration (JSON file), the managed host list (`hosts.json`) and the service links (`links.json`).

mod access_config;
mod link;
mod listen_check;
mod managed_host;
mod oidc_config;
mod online_source;
mod proxy_check;
mod proxy_config;
mod server_config;

pub use access_config::AccessConfig;
pub use link::{Link, LinkCheck, load_links};
pub use listen_check::is_tailscale_ip;
pub use managed_host::{ManagedHost, load_hosts};
pub use oidc_config::OidcConfig;
pub use online_source::OnlineSource;
pub use proxy_config::ProxyConfig;
pub use server_config::ServerConfig;
