//! One file per subcommand; `dispatch` routes a parsed command to it.

mod audit;
mod build;
mod dispatch;
mod log;
mod promote;
mod rebuild;
mod status;
mod updates;

pub use dispatch::run;
