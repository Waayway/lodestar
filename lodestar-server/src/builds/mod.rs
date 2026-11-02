//! Building every Linux host's system on the builder and publishing it to the cache.

mod builder;
mod building_guard;
mod channel_gate;
mod channel_heads;
mod gate;
mod prebuild;
mod result_file;

pub use builder::{Builder, HostBuild, RemoteBuilder};
pub use channel_gate::{build_channel, known_failures, skips_gate};
pub use channel_heads::ChannelHeads;
pub use gate::BuildGate;
pub use prebuild::{PrebuildQueue, prebuild_new_heads, spawn_prebuild};
pub(crate) use result_file::is_built_here;
pub use result_file::{BuildResult, HostResult};
