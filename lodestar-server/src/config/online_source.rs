use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Where host reachability comes from. `Static` is for tests/dev: always online.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OnlineSource {
    Tailscale,
    /// Host name -> IP. An address may carry a port (`127.0.0.1:54321`, tests/dev only)
    /// to reach an agent that is not on `agent_port`.
    Static {
        addresses: HashMap<String, String>,
    },
}
