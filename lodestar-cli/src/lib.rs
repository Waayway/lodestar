//! `lodestar`: a small CLI for the Lodestar server's HTTP API. Access is decided by the
//! server from the caller's Tailscale identity, so there are no tokens here.

use std::collections::BTreeMap;
use std::io::Write;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use lodestar_proto::{ApplyRecord, ApplyState, AuditEvent, HostView, OnlineState, Snapshot};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

#[derive(Parser)]
#[command(name = "lodestar", about = "Talk to the Lodestar server")]
pub struct Cli {
    /// Lodestar server URL (e.g. `http://manager:8080`). Falls back to `server = "..."` in
    /// `$XDG_CONFIG_HOME/lodestar/config.toml` (default `~/.config/lodestar/config.toml`).
    #[arg(long, env = "LODESTAR_SERVER")]
    pub server: Option<String>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Table of all hosts.
    Status,
    /// Force a rebuild: apply the channel's current head, even if the host already runs it.
    Rebuild {
        /// Host to rebuild (omit when using --channel).
        host: Option<String>,
        /// Rebuild every host on this channel instead.
        #[arg(long, conflicts_with = "host")]
        channel: Option<String>,
        /// Wait for the apply to finish; exit 0 only if it succeeded. Keeps retrying while the
        /// server is unreachable (e.g. restarting because the apply switched its host).
        #[arg(long)]
        wait: bool,
        /// Give up waiting after this many seconds.
        #[arg(long, default_value_t = 900)]
        timeout: u64,
    },
    /// Print an apply's log (default: the host's last apply).
    Log {
        host: String,
        apply_id: Option<String>,
    },
    /// Fast-forward the promote target channel (default `stable`) to the candidate.
    Promote {
        /// Actually promote (otherwise only show the candidate).
        #[arg(long)]
        yes: bool,
    },
    /// Recent audit events.
    Audit {
        #[arg(long, default_value_t = 20)]
        limit: u64,
    },
}

/// The config file the CLI reads when neither `--server` nor `LODESTAR_SERVER` is given.
pub fn config_path() -> Option<std::path::PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        Some(dir) => std::path::PathBuf::from(dir),
        None => std::path::PathBuf::from(std::env::var_os("HOME")?).join(".config"),
    };
    Some(base.join("lodestar").join("config.toml"))
}

/// Reads `server = "..."` from the CLI config file's text. Only top-level `key = "string"`
/// lines are understood; anything else (comments, other keys) is ignored.
pub fn server_from_config(text: &str) -> Option<String> {
    let mut in_table = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_table = true;
            continue;
        }
        if in_table || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "server" {
            continue;
        }
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.split_once('"'))
            .map(|(v, _)| v)
            .or_else(|| {
                value
                    .strip_prefix('\'')
                    .and_then(|v| v.split_once('\''))
                    .map(|(v, _)| v)
            })?;
        return Some(value.to_string()).filter(|v| !v.is_empty());
    }
    None
}

/// The server URL: `--server` / `LODESTAR_SERVER`, else the config file.
pub fn resolve_server(flag: Option<String>) -> Result<String> {
    if let Some(s) = flag.filter(|s| !s.is_empty()) {
        return Ok(s);
    }
    let path = config_path();
    if let Some(p) = &path
        && let Ok(text) = std::fs::read_to_string(p)
        && let Some(s) = server_from_config(&text)
    {
        return Ok(s);
    }
    bail!(
        "no server given: pass --server <url>, set LODESTAR_SERVER, or put `server = \"http://<host>:8080\"` in {}",
        path.map_or("~/.config/lodestar/config.toml".into(), |p| p
            .display()
            .to_string())
    )
}

/// Why a request to the server failed, for telling a restarting server from a real error.
#[derive(Debug)]
pub enum RequestError {
    /// No answer: connection refused or reset, timeout, or the body broke off.
    Unreachable(String),
    /// The server answered with an error status.
    Status(u16, String),
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RequestError::Unreachable(msg) => f.write_str(msg),
            RequestError::Status(403, msg) => write!(
                f,
                "not allowed: the server only accepts the owner's tailnet devices ({msg})"
            ),
            RequestError::Status(code, msg) => write!(f, "server answered {code}: {msg}"),
        }
    }
}

impl std::error::Error for RequestError {}

/// True for failures a server restart causes (no answer, or 502/503/504 from a proxy or a
/// server still starting); false for anything the server decided (403, 404, bad JSON, ...).
pub fn is_transient(e: &anyhow::Error) -> bool {
    match e.downcast_ref::<RequestError>() {
        Some(RequestError::Unreachable(_)) => true,
        Some(RequestError::Status(code, _)) => matches!(code, 502..=504),
        None => false,
    }
}

pub struct Client {
    base: String,
    http: reqwest::Client,
    /// How often `--wait` polls the snapshot.
    pub poll: Duration,
    /// First and longest pause between retries while `--wait` cannot reach the server.
    pub retry: (Duration, Duration),
}

impl Client {
    pub fn new(server: &str) -> Self {
        let mut headers = reqwest::header::HeaderMap::new();
        // CSRF guard on the server: every state-changing request needs this.
        headers.insert("x-lodestar", reqwest::header::HeaderValue::from_static("1"));
        let base = server.trim_end_matches('/');
        let base = if base.contains("://") {
            base.to_string()
        } else {
            format!("http://{base}")
        };
        Client {
            base,
            http: reqwest::Client::builder()
                .default_headers(headers)
                .timeout(Duration::from_secs(60))
                .build()
                .expect("reqwest client"),
            poll: Duration::from_secs(1),
            retry: (Duration::from_secs(2), Duration::from_secs(10)),
        }
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<reqwest::Response> {
        let resp = req.send().await.map_err(|e| {
            anyhow::Error::new(RequestError::Unreachable(format!(
                "cannot reach the Lodestar server at {} ({e}); is Tailscale up?",
                self.base
            )))
        })?;
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }
        let text = resp.text().await.unwrap_or_default();
        let msg = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v["error"].as_str().map(String::from))
            .unwrap_or(text);
        Err(RequestError::Status(status.as_u16(), msg).into())
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let resp = self
            .send(self.http.get(format!("{}{path}", self.base)))
            .await?;
        let body = resp.bytes().await.map_err(|e| {
            anyhow::Error::new(RequestError::Unreachable(format!(
                "reading the response of GET {path}: {e}"
            )))
        })?;
        serde_json::from_slice(&body).with_context(|| format!("decoding response of GET {path}"))
    }

    async fn post(&self, path: &str, body: Value) -> Result<Value> {
        let resp = self
            .send(self.http.post(format!("{}{path}", self.base)).json(&body))
            .await?;
        resp.json()
            .await
            .with_context(|| format!("decoding response of POST {path}"))
    }

    pub async fn snapshot(&self) -> Result<Snapshot> {
        self.get("/api/snapshot").await
    }

    async fn log_text(&self, host: &str, id: &str) -> Result<String> {
        let resp = self
            .send(
                self.http
                    .get(format!("{}/api/hosts/{host}/applies/{id}/log", self.base)),
            )
            .await?;
        Ok(resp.text().await?)
    }
}

fn short(s: &str) -> &str {
    &s[..s.len().min(8)]
}

fn state_str(s: &ApplyState) -> &'static str {
    match s {
        ApplyState::Idle => "idle",
        ApplyState::Running => "running",
        ApplyState::Ok => "ok",
        ApplyState::Skipped => "skipped",
        ApplyState::Failed => "failed",
    }
}

fn is_terminal(s: &ApplyState) -> bool {
    matches!(s, ApplyState::Ok | ApplyState::Skipped | ApplyState::Failed)
}

fn describe(a: &ApplyRecord) -> String {
    let mut s = state_str(&a.state).to_string();
    if let Some(r) = &a.skip_reason {
        s.push_str(&format!(
            " ({})",
            serde_json::to_string(r)
                .unwrap_or_default()
                .trim_matches('"')
        ));
    }
    if let Some(c) = a.exit_code.filter(|c| *c != 0) {
        s.push_str(&format!(" (exit {c})"));
    }
    if let Some(m) = &a.message {
        s.push_str(&format!(": {m}"));
    }
    s
}

fn find_host<'a>(snap: &'a Snapshot, name: &str) -> Result<&'a HostView> {
    snap.hosts
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| {
            let names: Vec<_> = snap.hosts.iter().map(|h| h.name.as_str()).collect();
            anyhow!("unknown host {name} (known: {})", names.join(", "))
        })
}

/// Runs a command, writing results to `out`. Returns the process exit code.
pub async fn run(client: &Client, cmd: Command, out: &mut impl Write) -> Result<i32> {
    match cmd {
        Command::Status => status(client, out).await,
        Command::Rebuild {
            host,
            channel,
            wait,
            timeout,
        } => {
            rebuild(
                client,
                host,
                channel,
                wait,
                Duration::from_secs(timeout),
                out,
            )
            .await
        }
        Command::Log { host, apply_id } => {
            let id = match apply_id {
                Some(id) => id,
                None => {
                    let snap = client.snapshot().await?;
                    let h = find_host(&snap, &host)?;
                    h.last_apply
                        .as_ref()
                        .map(|a| a.apply_id.clone())
                        .ok_or_else(|| anyhow!("{} has no apply yet", h.name))?
                }
            };
            write!(out, "{}", client.log_text(&host, &id).await?)?;
            Ok(0)
        }
        Command::Promote { yes } => promote(client, yes, out).await,
        Command::Audit { limit } => {
            let events: Vec<AuditEvent> = client.get(&format!("/api/audit?limit={limit}")).await?;
            for e in events {
                writeln!(
                    out,
                    "{}  {:<10} {:<18} {}  {}",
                    e.time,
                    e.host.as_deref().unwrap_or("-"),
                    e.kind,
                    e.actor,
                    e.details
                )?;
            }
            Ok(0)
        }
    }
}

async fn status(client: &Client, out: &mut impl Write) -> Result<i32> {
    let snap = client.snapshot().await?;
    let rows: Vec<[String; 7]> = snap
        .hosts
        .iter()
        .map(|h| {
            [
                h.name.clone(),
                h.os.clone(),
                h.channel.clone(),
                match h.online {
                    OnlineState::On => "online",
                    OnlineState::AgentUnreachable => "agent-down",
                    OnlineState::Off => "offline",
                }
                .into(),
                h.running.as_deref().map(short).unwrap_or("-").to_string()
                    + if h.running_dirty { "+dirty" } else { "" },
                h.behind.map_or("-".into(), |b| b.to_string()),
                h.last_apply.as_ref().map_or("-".into(), describe),
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
    let mut w = head.map(str::len);
    for r in &rows {
        for (i, c) in r.iter().enumerate() {
            w[i] = w[i].max(c.chars().count());
        }
    }
    let line = |cells: Vec<&str>| {
        cells
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{c:<w$}", w = w[i]))
            .collect::<Vec<_>>()
            .join("  ")
    };
    writeln!(out, "{}", line(head.to_vec()).trim_end())?;
    for r in &rows {
        writeln!(
            out,
            "{}",
            line(r.iter().map(String::as_str).collect()).trim_end()
        )?;
    }
    Ok(0)
}

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

async fn rebuild(
    client: &Client,
    host: Option<String>,
    channel: Option<String>,
    wait: bool,
    timeout: Duration,
    out: &mut impl Write,
) -> Result<i32> {
    // host name -> apply id
    let mut ids: BTreeMap<String, String> = BTreeMap::new();
    match (host, channel) {
        (Some(h), None) => {
            let r = client
                .post(&format!("/api/hosts/{h}/apply"), json!({}))
                .await?;
            let id = r["apply_id"].as_str().context("response has no apply_id")?;
            let st = r["agent_status"].as_str().unwrap_or("?");
            writeln!(
                out,
                "apply_id {id}\nagent_status {st}\n{}",
                agent_note(&h, st)
            )?;
            ids.insert(h, id.to_string());
        }
        (None, Some(c)) => {
            let r = client
                .post(&format!("/api/channels/{c}/apply"), json!({}))
                .await?;
            let map = r["apply_ids"]
                .as_object()
                .context("response has no apply_ids")?;
            for (h, id) in map {
                let st = r["agent_status"][h].as_str().unwrap_or("?");
                writeln!(
                    out,
                    "{h}: apply_id {} agent_status {st}",
                    id.as_str().unwrap_or("?")
                )?;
                if st == "unreachable" {
                    writeln!(out, "{}", agent_note(h, st))?;
                }
                ids.insert(h.clone(), id.as_str().unwrap_or_default().to_string());
            }
        }
        _ => bail!("give either a host or --channel <name>"),
    }
    if !wait {
        return Ok(0);
    }
    let deadline = Instant::now() + timeout;
    let mut done: BTreeMap<String, ApplyRecord> = BTreeMap::new();
    // While the server is unreachable (e.g. restarting because the apply switched its own
    // host): the next pause, and whether the outage was reported.
    let mut backoff = client.retry.0;
    let mut outage = false;
    let timed_out = |out: &mut dyn Write, done: &BTreeMap<String, ApplyRecord>| {
        let pending: Vec<_> = ids
            .keys()
            .filter(|h| !done.contains_key(*h))
            .cloned()
            .collect();
        writeln!(out, "timed out waiting for: {}", pending.join(", "))
    };
    while done.len() < ids.len() {
        let snap = match client.snapshot().await {
            Ok(snap) => {
                outage = false;
                backoff = client.retry.0;
                snap
            }
            Err(e) if is_transient(&e) => {
                if !outage {
                    writeln!(out, "server restarting, retrying… ({e:#})")?;
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
            Err(e) => return Err(e),
        };
        for (h, id) in &ids {
            if let Some(a) = find_host(&snap, h)?.last_apply.as_ref()
                && &a.apply_id == id
                && is_terminal(&a.state)
            {
                done.insert(h.clone(), a.clone());
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
    for (h, a) in &done {
        writeln!(out, "{h}: {}", describe(a))?;
        if a.state != ApplyState::Ok {
            code = 1;
        }
        match client.log_text(h, &a.apply_id).await {
            Ok(log) if !log.trim().is_empty() => {
                writeln!(out, "--- log {h} ---\n{}", log.trim_end())?
            }
            Ok(_) => {}
            Err(e) => writeln!(out, "(could not fetch log: {e:#})")?,
        }
    }
    Ok(code)
}

async fn promote(client: &Client, yes: bool, out: &mut impl Write) -> Result<i32> {
    let snap = client.snapshot().await?;
    let Some(pair) = snap.promote.clone() else {
        bail!("promote is disabled on this server");
    };
    let Some(commit) = snap.promote_candidate.clone() else {
        writeln!(out, "nothing to promote: {} is up to date", pair.to)?;
        return Ok(0);
    };
    let subject = snap
        .channels
        .iter()
        .find(|c| c.name == pair.from)
        .and_then(|c| {
            (c.head.as_deref() == Some(&commit))
                .then(|| c.head_subject.clone())
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
