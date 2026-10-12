//! Reads `applies/<id>.log` in chunks.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use lodestar_proto::{ApplyState, LogChunk};

use super::record::read_record;
use super::state_files::apply_file;
use crate::config::AgentConfig;

/// Log bytes from `offset` on. `eof` is true once the apply has finished and
/// the chunk reaches the end of the log. `offset` echoes the request; the next
/// request continues at `next` (the raw bytes consumed, which differs from
/// `data.len()` when invalid UTF-8 was replaced).
pub fn read_log(config: &AgentConfig, id: &str, offset: u64) -> Option<LogChunk> {
    let record = read_record(config, id)?;
    let mut buffer = Vec::new();
    let mut at_end = true;
    if let Ok(mut log_file) = File::open(apply_file(config, id, "log")) {
        let length = log_file.metadata().ok()?.len();
        if offset < length {
            log_file.seek(SeekFrom::Start(offset)).ok()?;
            log_file.read_to_end(&mut buffer).ok()?;
        }
        at_end = offset + buffer.len() as u64 >= length;
    }
    let eof = at_end && record.state != ApplyState::Running;
    let (data, consumed) = match std::str::from_utf8(&buffer) {
        Ok(text) => (text.to_string(), buffer.len()),
        // A multibyte char cut off by the end of a still-growing log: leave it
        // for the next read.
        Err(error) if !eof && error.error_len().is_none() => {
            let valid = error.valid_up_to();
            (
                String::from_utf8_lossy(&buffer[..valid]).into_owned(),
                valid,
            )
        }
        Err(_) => (String::from_utf8_lossy(&buffer).into_owned(), buffer.len()),
    };
    Some(LogChunk {
        apply_id: id.to_string(),
        offset,
        data,
        next: offset + consumed as u64,
        eof,
    })
}
