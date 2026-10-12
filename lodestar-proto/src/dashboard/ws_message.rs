use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{ChannelView, HostView, Snapshot};
use crate::LogChunk;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case", tag = "type", content = "data")]
#[ts(export)]
pub enum WsMessage {
    Snapshot(Snapshot),
    HostUpdated(Box<HostView>),
    ChannelsUpdated(Vec<ChannelView>),
    ApplyLog(LogChunk),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OnlineState;

    #[test]
    fn ws_message_is_tagged() {
        let host_view = HostView {
            name: "h".into(),
            os: "nixos".into(),
            channel: "stable".into(),
            online: OnlineState::On,
            last_seen: None,
            running: None,
            running_dirty: false,
            behind: None,
            checkout: None,
            last_apply: None,
            desired_apply_id: None,
            last_activation: None,
        };
        let json = serde_json::to_string(&WsMessage::HostUpdated(Box::new(host_view))).unwrap();
        assert!(json.contains("\"type\":\"host_updated\""), "{json}");
        assert!(json.contains("\"data\":{"), "{json}");
    }
}
