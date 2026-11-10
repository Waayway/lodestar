//! HTTP client for the Lodestar agent API.
//!
//! `addr` is the agent's `host:port` (see `poller::agent_base`).

use std::time::Duration;

use anyhow::Context;
use lodestar_proto::{ActivationsResponse, AgentStatus, ApplyRequest, LogChunk};
use reqwest::StatusCode;
use serde::de::DeserializeOwned;

const TIMEOUT: Duration = Duration::from_secs(10);
/// The most bytes of an agent answer the server reads (a log chunk is at most 1 MiB of text,
/// which JSON escaping can grow).
pub const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone)]
pub struct AgentClient {
    http: reqwest::Client,
}

impl AgentClient {
    pub fn new() -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(TIMEOUT)
            // An agent (or whatever answers at its address) must not send the server elsewhere.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("building HTTP client")?;
        Ok(Self { http })
    }

    async fn get_json<T: DeserializeOwned>(
        &self,
        addr: &str,
        path: &str,
        query: &[(&str, &str)],
    ) -> anyhow::Result<T> {
        let url = format!("http://{addr}{path}");
        let response = self
            .http
            .get(&url)
            .query(query)
            .send()
            .await
            .with_context(|| format!("GET {url}"))?
            .error_for_status()?;
        // Redirects are not followed, and not an answer either.
        anyhow::ensure!(
            response.status().is_success(),
            "GET {url}: agent answered {}",
            response.status()
        );
        let body = read_capped(response)
            .await
            .with_context(|| format!("reading {url}"))?;
        serde_json::from_slice(&body).with_context(|| format!("decoding {url}"))
    }

    pub async fn status(&self, addr: &str) -> anyhow::Result<AgentStatus> {
        self.get_json(addr, "/status", &[]).await
    }

    /// `202` started, `200` already ran, `409` busy, `400` bad request.
    pub async fn apply(&self, addr: &str, request: &ApplyRequest) -> anyhow::Result<StatusCode> {
        let url = format!("http://{addr}/apply");
        let response = self
            .http
            .post(&url)
            .json(request)
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;
        Ok(response.status())
    }

    pub async fn log(&self, addr: &str, apply_id: &str, offset: u64) -> anyhow::Result<LogChunk> {
        let offset = offset.to_string();
        self.get_json(
            addr,
            "/apply/log",
            &[("apply_id", apply_id), ("offset", &offset)],
        )
        .await
    }

    pub async fn activations(&self, addr: &str, since: u64) -> anyhow::Result<ActivationsResponse> {
        let since = since.to_string();
        self.get_json(addr, "/activations", &[("since", &since)])
            .await
    }
}

/// The body of `response`, refusing more than `MAX_RESPONSE_BYTES` (whether announced in
/// `Content-Length` or streamed without one).
async fn read_capped(mut response: reqwest::Response) -> anyhow::Result<Vec<u8>> {
    if let Some(length) = response.content_length() {
        anyhow::ensure!(
            length <= MAX_RESPONSE_BYTES as u64,
            "answer too large ({length} bytes, at most {MAX_RESPONSE_BYTES})"
        );
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        anyhow::ensure!(
            body.len() + chunk.len() <= MAX_RESPONSE_BYTES,
            "answer too large (over {MAX_RESPONSE_BYTES} bytes)"
        );
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}
