//! `rebuild --wait`: polls the snapshot until every apply has finished.

use std::collections::BTreeMap;
use std::io::Write;
use std::time::{Duration, Instant};

use anyhow::Result;
use lodestar_proto::{ApplyRecord, ApplyState};

use crate::client::Client;
use crate::display::{describe, is_terminal};
use crate::host_lookup::find_host;
use crate::request_error::is_transient;

/// Waits for the applies in `ids` (host name -> apply id), then prints each outcome and
/// log. Returns 0 when all succeeded, 1 on a failure or a timeout.
pub(crate) async fn wait_for_applies(
    client: &Client,
    ids: &BTreeMap<String, String>,
    timeout: Duration,
    out: &mut impl Write,
) -> Result<i32> {
    let deadline = Instant::now() + timeout;
    let mut done: BTreeMap<String, ApplyRecord> = BTreeMap::new();
    // While the server is unreachable (e.g. restarting because the apply switched its own
    // host): the next pause, and whether the outage was reported.
    let mut backoff = client.retry.0;
    let mut outage = false;
    let timed_out = |out: &mut dyn Write, done: &BTreeMap<String, ApplyRecord>| {
        let pending: Vec<_> = ids
            .keys()
            .filter(|host| !done.contains_key(*host))
            .cloned()
            .collect();
        writeln!(out, "timed out waiting for: {}", pending.join(", "))
    };
    while done.len() < ids.len() {
        // Every request is capped by what is left of --timeout (the client's own request
        // timeout is longer).
        let left = deadline.saturating_duration_since(Instant::now());
        let Ok(result) = tokio::time::timeout(left, client.snapshot()).await else {
            timed_out(out, &done)?;
            return Ok(1);
        };
        let snapshot = match result {
            Ok(snapshot) => {
                outage = false;
                backoff = client.retry.0;
                snapshot
            }
            Err(error) if is_transient(&error) => {
                if !outage {
                    writeln!(out, "server restarting, retrying… ({error:#})")?;
                    outage = true;
                }
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    timed_out(out, &done)?;
                    return Ok(1);
                }
                tokio::time::sleep(backoff.min(left)).await;
                backoff = (backoff * 2).min(client.retry.1);
                continue;
            }
            Err(error) => return Err(error),
        };
        for (host, id) in ids {
            if let Some(apply) = find_host(&snapshot, host)?.last_apply.as_ref()
                && &apply.apply_id == id
                && is_terminal(&apply.state)
            {
                done.insert(host.clone(), apply.clone());
            }
        }
        if done.len() == ids.len() {
            break;
        }
        if Instant::now() >= deadline {
            timed_out(out, &done)?;
            return Ok(1);
        }
        tokio::time::sleep(client.poll).await;
    }
    let mut code = 0;
    for (host, apply) in &done {
        writeln!(out, "{host}: {}", describe(apply))?;
        if apply.state != ApplyState::Ok {
            code = 1;
        }
        match client.log_text(host, &apply.apply_id).await {
            Ok(log) if !log.trim().is_empty() => {
                writeln!(out, "--- log {host} ---\n{}", log.trim_end())?
            }
            Ok(_) => {}
            Err(error) => writeln!(out, "(could not fetch log: {error:#})")?,
        }
    }
    Ok(code)
}
