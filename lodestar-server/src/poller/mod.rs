//! Polls every fleet host's agent: status, apply transitions, activations, re-sends of a
//! missed desired apply, live apply logs. Every change is broadcast to dashboard clients.

mod activation_ingest;
mod apply_recording;
mod host_poll;
mod log_follower;
mod poll_loop;
mod reachability;
pub mod service_check;

pub use host_poll::poll_once;
pub use log_follower::follow_log;
pub use poll_loop::spawn;
pub use reachability::agent_base;
