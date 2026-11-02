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
    /// moves to previous (also when `commit` was the previous one: the channel went back).
    pub fn advance(heads: Option<&ChannelHeads>, commit: &str) -> ChannelHeads {
        ChannelHeads {
            current: commit.to_string(),
            previous: heads
                .map(|heads| heads.current.clone())
                .filter(|current| current != commit),
        }
    }
}
