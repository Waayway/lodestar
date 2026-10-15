//! One HTTP probe of a service link and the classification of its failures.

mod probe_outcome;
mod probers;

use std::time::Duration;

pub use probe_outcome::ProbeOutcome;
pub use probers::{Probers, classify_error, probe};

/// How often every link is checked.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(60);
/// Per-request timeout (connect + response headers).
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(5);
/// Links probed at the same time.
pub const CHECK_CONCURRENCY: usize = 8;
