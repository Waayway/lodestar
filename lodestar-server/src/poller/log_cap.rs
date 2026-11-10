//! Bounds the apply log the server keeps of an agent, which it does not trust.

use anyhow::ensure;
use lodestar_proto::LogChunk;

use crate::agent_client::MAX_RESPONSE_BYTES;
use crate::state::AppState;

/// The most log text the server keeps (and stores) of one apply.
pub const MAX_LOG_BYTES: usize = 16 * 1024 * 1024;

/// The most log reads in a row without a pause (a full read, or a follower's burst through
/// a backlog). Consistent chunks reach `MAX_LOG_BYTES` long before this.
const MAX_LOG_READS: u32 = 64;

/// Ends a log cut at `MAX_LOG_BYTES`.
pub(super) const TRUNCATED: &str = "\n[lodestar: log truncated]\n";

/// The most bytes one agent log read answers (`lodestar_agent::apply::MAX_LOG_CHUNK`).
const AGENT_LOG_CHUNK: u64 = 1024 * 1024;

/// Whether a read of `consumed` bytes was a full agent chunk, so more is waiting. The agent
/// leaves a UTF-8 char cut by the chunk end (up to 3 bytes) for the next read.
pub(super) fn full_chunk(consumed: u64) -> bool {
    consumed >= AGENT_LOG_CHUNK - 3
}

/// What to do after a chunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Next {
    /// The log ended (eof) or reached the cap: stop reading.
    Done,
    /// A full chunk: more is waiting, read again now.
    More,
    /// The end so far: read again later.
    Wait,
}

/// An apply log read chunk by chunk, checking each chunk against the request and the bytes
/// it claims, up to `max_bytes` of text and `max_reads` reads in a row.
pub(super) struct LogCap {
    log: String,
    offset: u64,
    reads: u32,
    max_bytes: usize,
    max_reads: u32,
    truncated: bool,
}

impl LogCap {
    pub(super) fn new() -> Self {
        Self::with_limits(MAX_LOG_BYTES, MAX_LOG_READS)
    }

    pub(super) fn with_limits(max_bytes: usize, max_reads: u32) -> Self {
        Self {
            log: String::new(),
            offset: 0,
            reads: 0,
            max_bytes,
            max_reads,
            truncated: false,
        }
    }

    /// The agent's byte offset to read at next (not `log.len()`: replaced invalid UTF-8
    /// changes the length of the text).
    pub(super) fn offset(&self) -> u64 {
        self.offset
    }

    /// True when bytes over the cap were left out.
    pub(super) fn truncated(&self) -> bool {
        self.truncated
    }

    pub(super) fn log(&self) -> &str {
        &self.log
    }

    pub(super) fn into_log(self) -> String {
        self.log
    }

    /// Starts a new run of reads (after a pause).
    pub(super) fn reset_reads(&mut self) {
        self.reads = 0;
    }

    /// Takes the answer to a read at `offset()`. Refuses (and keeps nothing of) a chunk for
    /// another offset, one going back or claiming more than an answer can hold, and one whose
    /// text does not fit the bytes it claims: valid text is as long as its bytes, and each
    /// replaced invalid sequence (1 to 3 bytes) is 3 bytes of text.
    pub(super) fn accept(&mut self, chunk: &LogChunk) -> anyhow::Result<Next> {
        self.reads += 1;
        ensure!(
            self.reads <= self.max_reads,
            "more than {} log reads in a row",
            self.max_reads
        );
        ensure!(
            chunk.offset == self.offset,
            "log chunk for offset {}, asked for {}",
            chunk.offset,
            self.offset
        );
        ensure!(
            chunk.next >= self.offset,
            "log chunk goes back from {} to {}",
            self.offset,
            chunk.next
        );
        let consumed = chunk.next - self.offset;
        let text = chunk.data.len() as u64;
        ensure!(
            consumed <= MAX_RESPONSE_BYTES as u64 && consumed <= text && text <= consumed * 3,
            "log chunk claims {consumed} bytes for {text} bytes of text"
        );
        self.offset = chunk.next;
        let room = self.max_bytes.saturating_sub(self.log.len());
        if chunk.data.len() > room {
            let mut end = room;
            while !chunk.data.is_char_boundary(end) {
                end -= 1;
            }
            self.log.push_str(&chunk.data[..end]);
            self.log.push_str(TRUNCATED);
            self.truncated = true;
            return Ok(Next::Done);
        }
        self.log.push_str(&chunk.data);
        Ok(if chunk.eof {
            Next::Done
        } else if full_chunk(consumed) {
            Next::More
        } else {
            Next::Wait
        })
    }
}

/// Reads an apply's log from the start to its end so far, or to `MAX_LOG_BYTES`. Only a full
/// agent chunk is followed by another read, and never more than `MAX_LOG_READS` of them.
pub(crate) async fn read_log_capped(
    state: &AppState,
    base: &str,
    apply_id: &str,
) -> anyhow::Result<String> {
    let mut cap = LogCap::new();
    loop {
        let chunk = state.client.log(base, apply_id, cap.offset()).await?;
        if cap.accept(&chunk)? != Next::More {
            return Ok(cap.into_log());
        }
    }
}

#[cfg(test)]
mod tests {
    use lodestar_proto::LogChunk;

    use super::*;

    fn chunk(offset: u64, data: &str, next: u64, eof: bool) -> LogChunk {
        LogChunk {
            apply_id: "a1".into(),
            offset,
            data: data.into(),
            next,
            eof,
        }
    }

    #[test]
    fn a_log_of_exactly_the_cap_is_complete() {
        let mut cap = LogCap::with_limits(4, 10);
        assert_eq!(cap.accept(&chunk(0, "abcd", 4, true)).unwrap(), Next::Done);
        assert!(!cap.truncated());
        assert_eq!(cap.into_log(), "abcd");
        // Reaching the cap before eof is not a cut either: an empty eof chunk ends it.
        let mut cap = LogCap::with_limits(4, 10);
        assert_eq!(cap.accept(&chunk(0, "abcd", 4, false)).unwrap(), Next::Wait);
        assert_eq!(cap.accept(&chunk(4, "", 4, true)).unwrap(), Next::Done);
        assert_eq!(cap.into_log(), "abcd");
    }

    #[test]
    fn bytes_over_the_cap_cut_the_log_on_a_char_boundary() {
        let mut cap = LogCap::with_limits(4, 10);
        assert_eq!(cap.accept(&chunk(0, "abcd", 4, false)).unwrap(), Next::Wait);
        assert_eq!(cap.accept(&chunk(4, "e", 5, true)).unwrap(), Next::Done);
        assert!(cap.truncated());
        assert_eq!(cap.into_log(), format!("abcd{TRUNCATED}"));
        let mut cap = LogCap::with_limits(4, 10);
        assert_eq!(cap.accept(&chunk(0, "abcé", 5, true)).unwrap(), Next::Done);
        assert_eq!(cap.into_log(), format!("abc{TRUNCATED}"));
    }

    #[test]
    fn chunks_must_match_the_bytes_they_claim() {
        let mut cap = LogCap::new();
        // No data for 1 MiB claimed.
        assert!(cap.accept(&chunk(0, "", 1024 * 1024, false)).is_err());
        let mut cap = LogCap::new();
        // Not the offset asked for.
        assert!(cap.accept(&chunk(7, "x", 8, false)).is_err());
        let mut cap = LogCap::new();
        // Going back, or far beyond anything an answer can hold.
        assert!(cap.accept(&chunk(0, "x", 1, false)).is_ok());
        assert!(cap.accept(&chunk(1, "", 0, false)).is_err());
        let mut cap = LogCap::new();
        assert!(cap.accept(&chunk(0, "x", u64::MAX, false)).is_err());
        let mut cap = LogCap::new();
        // Data without progress.
        assert!(cap.accept(&chunk(0, "x", 0, false)).is_err());
        // Replaced invalid bytes: 1 raw byte, 3 bytes of text.
        let mut cap = LogCap::new();
        assert_eq!(
            cap.accept(&chunk(0, "\u{fffd}", 1, false)).unwrap(),
            Next::Wait
        );
    }

    #[test]
    fn reads_are_capped_however_the_chunks_look() {
        let mut cap = LogCap::with_limits(MAX_LOG_BYTES, 2);
        assert!(cap.accept(&chunk(0, "a", 1, false)).is_ok());
        assert!(cap.accept(&chunk(1, "b", 2, false)).is_ok());
        assert!(cap.accept(&chunk(2, "c", 3, false)).is_err());
        cap.reset_reads();
        assert!(cap.accept(&chunk(2, "c", 3, false)).is_ok());
    }

    #[test]
    fn a_full_chunk_allows_a_deferred_char_and_never_overflows() {
        assert!(full_chunk(1024 * 1024));
        assert!(full_chunk(1024 * 1024 - 3));
        assert!(!full_chunk(1024 * 1024 - 4));
        assert!(!full_chunk(9));
        assert!(full_chunk(u64::MAX));
    }
}
