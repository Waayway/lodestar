//! Types the server sends to the dashboard.

mod audit_event;
mod channel_view;
mod host_view;
mod me;
mod promote_channels;
mod public_service;
mod role;
mod service_view;
mod snapshot;
mod ws_message;

pub use audit_event::AuditEvent;
pub use channel_view::ChannelView;
pub use host_view::{HostView, OnlineState};
pub use me::{ActDenied, Me};
pub use promote_channels::PromoteChannels;
pub use public_service::{PublicService, PublicServiceState};
pub use role::Role;
pub use service_view::{DownReason, ServiceCheck, ServiceState, ServiceView};
pub use snapshot::Snapshot;
pub use ws_message::WsMessage;
