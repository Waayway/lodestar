//! `lodestar status`.

use std::io::Write;

use anyhow::Result;
use lodestar_proto::OnlineState;

use crate::client::Client;
use crate::display::{describe, short};
use crate::table::write_table;

pub(super) async fn status(client: &Client, out: &mut impl Write) -> Result<i32> {
    let snapshot = client.snapshot().await?;
    let rows: Vec<[String; 7]> = snapshot
        .hosts
        .iter()
        .map(|host| {
            [
                host.name.clone(),
                host.os.clone(),
                host.channel.clone(),
                match host.online {
                    OnlineState::On => "online",
                    OnlineState::AgentUnreachable => "agent-down",
                    OnlineState::Off => "offline",
                }
                .into(),
                host.running
                    .as_deref()
                    .map(short)
                    .unwrap_or("-")
                    .to_string()
                    + if host.running_dirty { "+dirty" } else { "" },
                host.behind.map_or("-".into(), |behind| behind.to_string()),
                host.last_apply.as_ref().map_or("-".into(), describe),
            ]
        })
        .collect();
    let head = [
        "HOST",
        "OS",
        "CHANNEL",
        "ONLINE",
        "RUNNING",
        "BEHIND",
        "LAST APPLY",
    ];
    write_table(out, head, &rows)?;
    Ok(0)
}
