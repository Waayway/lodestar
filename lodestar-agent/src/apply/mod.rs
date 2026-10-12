//! The apply wrapper body and the state files it leaves for the agent daemon.
//!
//! Layout under `<state_dir>`: `apply-in-progress.json` while an apply runs,
//! and `applies/<id>.{json,log,pid}` per apply.

mod apply_log;
mod lock;
mod log_reader;
mod marker;
mod outcome;
mod prune;
mod record;
mod state_files;
mod wrapper;

pub use lock::try_lock_apply;
pub use log_reader::read_log;
pub use marker::clear_stale_marker;
pub use prune::prune;
pub use record::{latest_record, read_record, settle_running};
pub use state_files::{applies_dir, is_valid_apply_id, record_fetch};
pub use wrapper::run_apply;
