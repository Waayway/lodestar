use lodestar_proto::{DownReason, ServiceState};

/// The result of probing one link once.
#[derive(Debug, Clone, PartialEq)]
pub struct ProbeOutcome {
    pub state: ServiceState,
    pub reason: Option<DownReason>,
    pub latency_ms: Option<u32>,
}
