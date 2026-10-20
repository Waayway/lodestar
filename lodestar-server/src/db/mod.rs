//! SQLite persistence (SeaORM). Enums are stored as their snake_case serde string.

mod activations;
mod applies;
mod audit;
mod connection;
mod desired;
pub mod entities;
mod sessions;
mod status;

pub use activations::{activations_next, set_activations_next};
pub use applies::{get_apply, insert_apply, set_apply_log, update_apply_from_record};
pub use audit::{insert_audit, last_audit, recent_audit};
pub use connection::connect;
pub use desired::{desired, set_desired};
pub use sessions::{delete_expired_sessions, delete_session, find_session, insert_session};
pub use status::{load_status, save_status};
