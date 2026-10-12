//! Host reachability and viewer identity, from the `tailscale` CLI.

mod cli_provider;
mod identity;
mod online_provider;
mod peer_info;
mod static_provider;
mod status_parser;
mod whois_parser;

pub use cli_provider::TailscaleCli;
pub use identity::Identity;
pub use online_provider::{OnlineProvider, provider_from_config};
pub use peer_info::PeerInfo;
pub use static_provider::{StaticOnline, parse_static_addr};
pub use status_parser::parse_status;
pub use whois_parser::parse_whois;
