//! Shared wire types for the Lodestar agent, server and web frontend.
//! Times are unix seconds (`i64`).

mod agent;
mod commit_hash;
mod dashboard;

pub use agent::{
    ActivationEvent, ActivationsResponse, AgentStatus, AgentToken, ApplyRecord, ApplyRequest,
    ApplyState, CheckoutStatus, LogChunk, SkipReason, bearer_token,
};
pub use commit_hash::is_commit_hash;
pub use dashboard::{
    ActDenied, AuditEvent, BuildView, BuildsView, ChannelBuildView, ChannelView, DownReason,
    HostBuildView, HostView, InputView, Me, OnlineState, PromoteChannels, PublicService,
    PublicServiceState, Role, ServiceCheck, ServiceState, ServiceView, Snapshot, SoftwareView,
    UpdateRunView, UpdatesView, VersionChangeView, WsMessage,
};
