//! `lodestar rebuild`.

use std::collections::BTreeMap;
use std::io::Write;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use lodestar_proto::ChannelView;
use serde_json::json;

use crate::client::Client;
use crate::wait::wait_for_applies;

fn agent_note(host: &str, status: &str) -> String {
    match status {
        "202" => format!("{host}: agent started the apply"),
        "200" => format!("{host}: agent had already run this apply"),
        "409" => format!("{host}: agent is busy with another apply; the server will retry"),
        "unreachable" => format!(
            "{host}: agent unreachable; the apply is recorded and the server will re-send it when the host comes online"
        ),
        other => format!("{host}: agent answered {other}"),
    }
}

/// The commit an apply on `channel` (of `host`, when given) will wait for: the channel is
/// building, or the gate is on (some channel has a build) and the channel's head has no result
/// yet. `None` when nothing is known or nothing waits (darwin hosts never wait).
async fn awaited_build(
    client: &Client,
    host: Option<&str>,
    channel: Option<&str>,
) -> Option<String> {
    let channel = match host {
        Some(host) => {
            let snapshot = client.snapshot().await.ok()?;
            let view = snapshot
                .hosts
                .into_iter()
                .find(|view| view.name.eq_ignore_ascii_case(host))?;
            if view.os == "darwin" {
                return None;
            }
            view.channel
        }
        None => channel?.to_string(),
    };
    let channels: Vec<ChannelView> = client.get("/api/builds").await.ok()?;
    let gate_on = channels.iter().any(|view| view.build.is_some());
    let view = channels.into_iter().find(|view| view.name == channel)?;
    match view.build {
        Some(build) if build.state == "building" => Some(build.commit),
        None if gate_on => view.head,
        _ => None,
    }
}

pub(super) async fn rebuild(
    client: &Client,
    host: Option<String>,
    channel: Option<String>,
    wait: bool,
    timeout: Duration,
    out: &mut impl Write,
) -> Result<i32> {
    // host name -> apply id
    let mut ids: BTreeMap<String, String> = BTreeMap::new();
    if host.is_some() != channel.is_some()
        && let Some(commit) = awaited_build(client, host.as_deref(), channel.as_deref()).await
    {
        let short = commit.get(..7).unwrap_or(&commit);
        writeln!(out, "waiting for the build of {short} on the builder…")?;
        out.flush()?;
    }
    match (host, channel) {
        (Some(host), None) => {
            let response = client
                .post_apply(&format!("/api/hosts/{host}/apply"), json!({}))
                .await?;
            let id = response["apply_id"]
                .as_str()
                .context("response has no apply_id")?;
            let agent_status = response["agent_status"].as_str().unwrap_or("?");
            writeln!(
                out,
                "apply_id {id}\nagent_status {agent_status}\n{}",
                agent_note(&host, agent_status)
            )?;
            ids.insert(host, id.to_string());
        }
        (None, Some(channel)) => {
            let response = client
                .post_apply(&format!("/api/channels/{channel}/apply"), json!({}))
                .await?;
            let apply_ids = response["apply_ids"]
                .as_object()
                .context("response has no apply_ids")?;
            for (host, id) in apply_ids {
                let agent_status = response["agent_status"][host].as_str().unwrap_or("?");
                writeln!(
                    out,
                    "{host}: apply_id {} agent_status {agent_status}",
                    id.as_str().unwrap_or("?")
                )?;
                if agent_status == "unreachable" {
                    writeln!(out, "{}", agent_note(host, agent_status))?;
                }
                ids.insert(host.clone(), id.as_str().unwrap_or_default().to_string());
            }
        }
        _ => bail!("give either a host or --channel <name>"),
    }
    if !wait {
        return Ok(0);
    }
    wait_for_applies(client, &ids, timeout, out).await
}
