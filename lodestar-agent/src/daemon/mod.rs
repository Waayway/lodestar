//! The agent daemon: binds the API and runs the background fetch.

mod background_fetch;
mod manager_addresses;
mod serve;

pub use serve::serve;
