use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct LogChunk {
    pub apply_id: String,
    /// Byte offset in the log this chunk starts at (echoes the request).
    #[ts(type = "number")]
    pub offset: u64,
    /// The bytes read, as text. Invalid UTF-8 is replaced, so `data`'s length need not match
    /// the bytes consumed: continue at `next`.
    pub data: String,
    /// Byte offset to request next: `offset` plus the raw log bytes this chunk consumed.
    #[ts(type = "number")]
    pub next: u64,
    pub eof: bool,
}
