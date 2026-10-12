//! One file per subcommand; `dispatch` routes a parsed command to it.

mod audit;
mod dispatch;
mod log;
mod promote;
mod rebuild;
mod status;

pub use dispatch::run;
