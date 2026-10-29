//! `lodestar updates` and `lodestar updates run`.

use std::io::Write;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use lodestar_proto::UpdatesView;
use serde_json::json;

use crate::cli::UpdatesAction;
use crate::client::Client;
use crate::display::age;
use crate::request_error::is_transient;
use crate::table::write_table;

pub(super) async fn updates(
    client: &Client,
    action: Option<UpdatesAction>,
    out: &mut impl Write,
) -> Result<i32> {
    match action {
        None => table(client, out).await,
        Some(UpdatesAction::Run { wait, timeout }) => {
            run(client, wait, Duration::from_secs(timeout), out).await
        }
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

async fn table(client: &Client, out: &mut impl Write) -> Result<i32> {
    let view: UpdatesView = client.get("/api/updates").await?;
    let now = now();
    let rows: Vec<[String; 5]> = view
        .software
        .iter()
        .map(|software| {
            let last = view
                .inputs
                .iter()
                .find(|input| input.name == software.input)
                .and_then(|input| input.last.as_ref());
            [
                software.name.clone(),
                software.latest.clone().unwrap_or_else(|| "-".into()),
                software.input.clone(),
                last.map_or("-".into(), |run| run.result.clone()),
                last.map_or("-".into(), |run| age(now - run.finished_at)),
            ]
        })
        .collect();
    write_table(
        out,
        ["SOFTWARE", "LATEST", "INPUT", "LAST RESULT", "AGE"],
        &rows,
    )?;
    if view.running {
        writeln!(out, "a run is in progress")?;
    }
    Ok(0)
}

async fn run(client: &Client, wait: bool, timeout: Duration, out: &mut impl Write) -> Result<i32> {
    client.post("/api/updates/run", json!({})).await?;
    writeln!(out, "update run started")?;
    if !wait {
        return Ok(0);
    }
    let deadline = Instant::now() + timeout;
    // The server may restart under us (an update can switch its own host): retry quietly.
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match tokio::time::timeout(left, client.get::<UpdatesView>("/api/updates")).await {
            Err(_) => {
                writeln!(out, "timed out waiting for the update run")?;
                return Ok(1);
            }
            Ok(Err(error)) if is_transient(&error) => {}
            Ok(Err(error)) => return Err(error),
            Ok(Ok(view)) => {
                if !view.running {
                    return finish(&view, out);
                }
            }
        }
        if Instant::now() >= deadline {
            writeln!(out, "timed out waiting for the update run")?;
            return Ok(1);
        }
        tokio::time::sleep(client.poll).await;
    }
}

/// Prints each input's last result; 0 only when every one is `pushed` or `no-change`.
fn finish(view: &UpdatesView, out: &mut impl Write) -> Result<i32> {
    let mut code = 0;
    for input in &view.inputs {
        match &input.last {
            Some(run) => {
                let host = run
                    .host
                    .as_deref()
                    .map_or(String::new(), |host| format!(" ({host})"));
                writeln!(out, "{}: {}{host}", input.name, run.result)?;
                if !matches!(run.result.as_str(), "pushed" | "no-change") {
                    code = 1;
                }
            }
            None => {
                writeln!(out, "{}: no result", input.name)?;
                code = 1;
            }
        }
    }
    Ok(code)
}
