//! Demo mode: the real server, poller, database, git mirror and dashboard against a
//! simulated set of hosts (a throwaway git repo and in-process simulated agents). All demo
//! timings are divided by `speed`.

mod agents;
mod demo_handle;
mod demo_host;
mod demo_online;
mod demo_peer;
mod repo;
mod start;
mod tasks;

pub use demo_handle::DemoHandle;
pub use start::start;
