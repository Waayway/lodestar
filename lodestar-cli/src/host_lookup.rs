use anyhow::{Result, anyhow};
use lodestar_proto::{HostView, Snapshot};

/// The host called `name` (case-insensitive), or an error listing the known hosts.
pub(crate) fn find_host<'a>(snapshot: &'a Snapshot, name: &str) -> Result<&'a HostView> {
    snapshot
        .hosts
        .iter()
        .find(|host| host.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| {
            let names: Vec<_> = snapshot
                .hosts
                .iter()
                .map(|host| host.name.as_str())
                .collect();
            anyhow!("unknown host {name} (known: {})", names.join(", "))
        })
}
