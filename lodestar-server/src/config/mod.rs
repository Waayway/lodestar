//! Server configuration (JSON file) and the managed host list (`hosts.json`).

mod access_config;
mod listen_check;
mod managed_host;
mod online_source;
mod server_config;

pub use access_config::AccessConfig;
pub use listen_check::is_tailscale_ip;
pub use managed_host::{ManagedHost, load_hosts};
pub use online_source::OnlineSource;
pub use server_config::ServerConfig;
