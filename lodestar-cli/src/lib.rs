//! `lodestar`: a small CLI for the Lodestar server's HTTP API. Access is decided by the
//! server from the caller's Tailscale identity, so there are no tokens here.

mod cli;
mod client;
mod commands;
mod display;
mod host_lookup;
mod request_error;
mod server_address;
mod table;
mod wait;

pub use cli::{Cli, Command};
pub use client::Client;
pub use commands::run;
pub use request_error::{RequestError, is_transient};
pub use server_address::{config_path, resolve_server, server_from_config};
