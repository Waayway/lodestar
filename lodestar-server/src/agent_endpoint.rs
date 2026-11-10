use std::fmt;

/// A host's agent: the managed host's name (which picks the token) and its `host:port`
/// (see `poller::agent_base`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentEndpoint {
    pub host: String,
    pub addr: String,
}

impl fmt::Display for AgentEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.addr)
    }
}
