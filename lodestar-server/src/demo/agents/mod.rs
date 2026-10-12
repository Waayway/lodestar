//! Simulated agents: one in-process HTTP server per demo host, implementing the real
//! agent API (`GET /status`, `POST /apply`, `GET /apply/log`, `GET /activations`) with the
//! agent's status codes. An apply streams a realistic log over ~10 s (divided by the demo
//! speed) and ends as the host's script says.

mod apply_run;
mod host_setup;
mod routes;
mod script;
mod sim_agent;
mod store_paths;

pub use host_setup::HostSetup;
pub use routes::serve;
pub use script::Script;
pub use sim_agent::SimAgent;
