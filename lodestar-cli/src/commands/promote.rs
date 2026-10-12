//! `lodestar promote`.

use std::io::Write;

use anyhow::{Result, bail};
use serde_json::json;

use crate::client::Client;
use crate::display::short;

pub(super) async fn promote(client: &Client, yes: bool, out: &mut impl Write) -> Result<i32> {
    let snapshot = client.snapshot().await?;
    let Some(pair) = snapshot.promote.clone() else {
        bail!("promote is disabled on this server");
    };
    let Some(commit) = snapshot.promote_candidate.clone() else {
        writeln!(out, "nothing to promote: {} is up to date", pair.to)?;
        return Ok(0);
    };
    let subject = snapshot
        .channels
        .iter()
        .find(|channel| channel.name == pair.from)
        .and_then(|channel| {
            (channel.head.as_deref() == Some(&commit))
                .then(|| channel.head_subject.clone())
                .flatten()
        });
    writeln!(
        out,
        "promote candidate ({} -> {}): {} {}",
        pair.from,
        pair.to,
        short(&commit),
        subject.unwrap_or_default()
    )?;
    if !yes {
        writeln!(out, "re-run with --yes to move {} to this commit", pair.to)?;
        return Ok(1);
    }
    client
        .post("/api/promote", json!({ "commit": commit }))
        .await?;
    writeln!(out, "{} promoted to {}", pair.to, short(&commit))?;
    Ok(0)
}
