//! `lodestar log`.

use std::io::Write;

use anyhow::{Result, anyhow};

use crate::client::Client;
use crate::host_lookup::find_host;

pub(super) async fn log(
    client: &Client,
    host: String,
    apply_id: Option<String>,
    out: &mut impl Write,
) -> Result<i32> {
    let id = match apply_id {
        Some(id) => id,
        None => {
            let snapshot = client.snapshot().await?;
            let host_view = find_host(&snapshot, &host)?;
            host_view
                .last_apply
                .as_ref()
                .map(|apply| apply.apply_id.clone())
                .ok_or_else(|| anyhow!("{} has no apply yet", host_view.name))?
        }
    };
    write!(out, "{}", client.log_text(&host, &id).await?)?;
    Ok(0)
}
