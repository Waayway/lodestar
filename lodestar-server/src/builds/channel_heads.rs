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
    /// `commit` became the channel's newest built commit; the old current one moves to previous
    /// (unchanged when it is the same commit).
    pub fn advance(heads: Option<&ChannelHeads>, commit: &str) -> ChannelHeads {
        match heads {
            Some(heads) if heads.current == commit => heads.clone(),
            Some(heads) => ChannelHeads {
                current: commit.to_string(),
                previous: Some(heads.current.clone()),
            },
            None => ChannelHeads {
                current: commit.to_string(),
                previous: None,
            },
        }
    }
}
