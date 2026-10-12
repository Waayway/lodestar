//! Reads the activation audit file written by the activation audit hook.

use std::path::Path;

use lodestar_proto::{ActivationEvent, ActivationsResponse};
use serde::Deserialize;

#[derive(Deserialize)]
struct RawEvent {
    time: i64,
    store_path: String,
    revision: String,
    #[serde(default)]
    generation: Option<u64>,
    trigger: String,
    #[serde(default)]
    sudo_user: Option<String>,
}

/// Events on lines `>= since` (0-based). Malformed lines are skipped and a
/// trailing line without `\n` (still being written) is left for next time.
/// `next` is the number of complete lines in the file.
pub fn read_since(path: &Path, since: u64) -> ActivationsResponse {
    let data = std::fs::read(path).unwrap_or_default();
    let complete = match data.iter().rposition(|byte| *byte == b'\n') {
        Some(index) => &data[..index],
        None => {
            return ActivationsResponse {
                events: vec![],
                next: 0,
            };
        }
    };
    let mut events = Vec::new();
    let mut next = 0u64;
    for (line_number, line) in complete.split(|byte| *byte == b'\n').enumerate() {
        let line_number = line_number as u64;
        next = line_number + 1;
        if line_number < since {
            continue;
        }
        if let Ok(raw) = serde_json::from_slice::<RawEvent>(line) {
            events.push(ActivationEvent {
                line: line_number,
                time: raw.time,
                store_path: raw.store_path,
                revision: raw.revision,
                generation: raw.generation,
                trigger: raw.trigger,
                sudo_user: raw.sudo_user,
            });
        }
    }
    ActivationsResponse { events, next }
}
