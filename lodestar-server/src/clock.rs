use std::time::{SystemTime, UNIX_EPOCH};

/// The current time in unix seconds (0 if the clock is before the epoch).
pub(crate) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64)
}
