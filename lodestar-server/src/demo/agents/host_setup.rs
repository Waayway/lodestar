use super::Script;

/// A simulated host as its agent sees it.
pub struct HostSetup {
    pub name: String,
    pub os: String,
    pub channel: String,
    pub user: String,
    pub running: String,
    pub script: Script,
    /// Generation of the running system.
    pub generation: u64,
    /// When the initial manual activation happened (unix seconds).
    pub activated_at: i64,
}
