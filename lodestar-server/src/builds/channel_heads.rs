//! One entry of `state_dir/builds/channels.json`, the gate's index of built channel heads (sent
//! to the publish command so the cache keeps what those commits need).

use serde::{Deserialize, Serialize};

/// The last two commits the gate wrote a result for on one channel.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ChannelHeads {
    pub current: String,
    pub previous: Option<String>,
}

impl ChannelHeads {
    /// The entry after `commit` became the channel's newest good build: the old current commit
    /// moves to previous. `None` when `commit` is already current or previous (a retry's rebuild
    /// does not move the channel).
    pub fn advance(heads: Option<&ChannelHeads>, commit: &str) -> Option<ChannelHeads> {
        match heads {
            Some(heads) if heads.current == commit || heads.previous.as_deref() == Some(commit) => {
                None
            }
            Some(heads) => Some(ChannelHeads {
                current: commit.to_string(),
                previous: Some(heads.current.clone()),
            }),
            None => Some(ChannelHeads {
                current: commit.to_string(),
                previous: None,
            }),
        }
    }
}
