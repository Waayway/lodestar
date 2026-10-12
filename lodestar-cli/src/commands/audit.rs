//! `lodestar audit`.

use std::io::Write;

use anyhow::Result;
use lodestar_proto::AuditEvent;

use crate::client::Client;

pub(super) async fn audit(client: &Client, limit: u64, out: &mut impl Write) -> Result<i32> {
    let events: Vec<AuditEvent> = client.get(&format!("/api/audit?limit={limit}")).await?;
    for event in events {
        writeln!(
            out,
            "{}  {:<10} {:<18} {}  {}",
            event.time,
            event.host.as_deref().unwrap_or("-"),
            event.kind,
            event.actor,
            event.details
        )?;
    }
    Ok(0)
}
