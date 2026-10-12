//! Shared wire types for the Lodestar agent, server and web frontend.
//! Times are unix seconds (`i64`).

mod agent;
mod commit_hash;
mod dashboard;

pub use agent::{
    ActivationEvent, ActivationsResponse, AgentStatus, ApplyRecord, ApplyRequest, ApplyState,
    CheckoutStatus, LogChunk, SkipReason,
};
pub use commit_hash::is_commit_hash;
pub use dashboard::{
    AuditEvent, ChannelView, HostView, OnlineState, PromoteChannels, Snapshot, WsMessage,
};
