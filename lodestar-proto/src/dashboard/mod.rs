//! Types the server sends to the dashboard.

mod audit_event;
mod channel_view;
mod host_view;
mod promote_channels;
mod service_view;
mod snapshot;
mod ws_message;

pub use audit_event::AuditEvent;
pub use channel_view::ChannelView;
pub use host_view::{HostView, OnlineState};
pub use promote_channels::PromoteChannels;
pub use service_view::{DownReason, ServiceCheck, ServiceState, ServiceView};
pub use snapshot::Snapshot;
pub use ws_message::WsMessage;
