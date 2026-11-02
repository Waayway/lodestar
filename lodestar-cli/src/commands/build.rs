//! `lodestar build status` and `lodestar build retry <commit>`.

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use lodestar_proto::ChannelView;
use serde_json::{Value, json};

use crate::cli::BuildAction;
use crate::client::Client;
use crate::display::age;
use crate::table::write_table;

pub(super) async fn build(
    client: &Client,
    action: BuildAction,
    out: &mut impl Write,
) -> Result<i32> {
    match action {
        BuildAction::Status => status(client, out).await,
        BuildAction::Retry { commit } => retry(client, &commit, out).await,
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

async fn status(client: &Client, out: &mut impl Write) -> Result<i32> {
    let channels: Vec<ChannelView> = client.get("/api/builds").await?;
    let now = now();
    let rows: Vec<[String; 5]> = channels
        .iter()
        .map(|channel| {
            let commit = channel
                .build
                .as_ref()
                .map(|build| build.commit.as_str())
                .or(channel.head.as_deref());
            let build = channel.build.as_ref();
            [
                channel.name.clone(),
                commit.map_or("-".into(), |commit| {
                    commit[..commit.len().min(7)].to_string()
                }),
                build.map_or("-".into(), |build| build.state.clone()),
                build
                    .filter(|build| !build.failed_hosts.is_empty())
                    .map_or("-".into(), |build| build.failed_hosts.join(", ")),
                build
                    .and_then(|build| build.finished_at)
                    .map_or("-".into(), |at| age(now - at)),
            ]
        })
        .collect();
    write_table(
        out,
        ["CHANNEL", "COMMIT", "STATE", "FAILED HOSTS", "AGE"],
        &rows,
    )?;
    Ok(0)
}

async fn retry(client: &Client, commit: &str, out: &mut impl Write) -> Result<i32> {
    let answer: Value = client
        .post(&format!("/api/builds/{commit}/retry"), json!({}))
        .await?;
    let hosts: Vec<&str> = answer["retried"]
        .as_array()
        .map(|hosts| hosts.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if hosts.is_empty() {
        writeln!(out, "nothing to retry: no host failed at {commit}")?;
    } else {
        writeln!(out, "retrying {}", hosts.join(", "))?;
    }
    Ok(0)
}
