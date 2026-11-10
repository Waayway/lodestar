//! HTTP client for the Lodestar agent API.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use lodestar_proto::{ActivationsResponse, AgentStatus, AgentToken, ApplyRequest, LogChunk};
use reqwest::StatusCode;
use reqwest::header::{AUTHORIZATION, HeaderValue};
use reqwest::redirect::Policy;
use serde::de::DeserializeOwned;

pub use crate::agent_endpoint::AgentEndpoint;

const TIMEOUT: Duration = Duration::from_secs(10);

/// Calls agents with their host's token (`Authorization: Bearer`, read from the host's token
/// file on every call, so a rotation needs no restart). Never follows a redirect, so the token
/// only goes to the agent's own address.
#[derive(Clone)]
pub struct AgentClient {
    http: reqwest::Client,
    /// Token file per host name.
    token_files: Arc<BTreeMap<String, PathBuf>>,
}

impl AgentClient {
    /// Fails when one of `hosts` has no token file, or any token file is unusable (missing,
    /// not a private regular file, not exactly one canonical token), so the server never
    /// starts with a broken token. Each call still re-reads its file (rotation).
    pub fn new<'a>(
        token_files: BTreeMap<String, PathBuf>,
        hosts: impl IntoIterator<Item = &'a str>,
    ) -> anyhow::Result<Self> {
        let missing: Vec<&str> = hosts
            .into_iter()
            .filter(|host| !token_files.contains_key(*host))
            .collect();
        anyhow::ensure!(
            missing.is_empty(),
            "no agent token file (agent_token_files) for {}; every managed host needs one",
            missing.join(", ")
        );
        for (host, path) in &token_files {
            AgentToken::read_one(path).with_context(|| format!("agent token for {host}"))?;
        }
        let http = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .redirect(Policy::none())
            // No proxy from the environment (HTTP_PROXY, ALL_PROXY, ...): the token goes
            // to the agent only.
            .no_proxy()
            .build()
            .context("building HTTP client")?;
        Ok(Self {
            http,
            token_files: Arc::new(token_files),
        })
    }

    /// The `Authorization` header for `agent` (marked sensitive, so it is never logged).
    fn authorization(&self, agent: &AgentEndpoint) -> anyhow::Result<HeaderValue> {
        let path = self
            .token_files
            .get(&agent.host)
            .with_context(|| format!("no agent token file for {}", agent.host))?;
        let token = AgentToken::read_one(path)
            .with_context(|| format!("agent token for {}", agent.host))?;
        let mut value = HeaderValue::from_str(&token.authorization())
            .context("agent token is not a valid header value")?;
        value.set_sensitive(true);
        Ok(value)
    }

    async fn get_json<T: DeserializeOwned>(
        &self,
        agent: &AgentEndpoint,
        path: &str,
        query: &[(&str, &str)],
    ) -> anyhow::Result<T> {
        let authorization = self.authorization(agent)?;
        let url = format!("http://{}{path}", agent.addr);
        let response = self
            .http
            .get(&url)
            .header(AUTHORIZATION, authorization)
            .query(query)
            .send()
            .await
            .with_context(|| format!("GET {url}"))?
            .error_for_status()?;
        response
            .json()
            .await
            .with_context(|| format!("decoding {url}"))
    }

    pub async fn status(&self, agent: &AgentEndpoint) -> anyhow::Result<AgentStatus> {
        self.get_json(agent, "/status", &[]).await
    }

    /// `202` started, `200` already ran, `409` busy, `400` bad request.
    pub async fn apply(
        &self,
        agent: &AgentEndpoint,
        request: &ApplyRequest,
    ) -> anyhow::Result<StatusCode> {
        let authorization = self.authorization(agent)?;
        let url = format!("http://{}/apply", agent.addr);
        let response = self
            .http
            .post(&url)
            .header(AUTHORIZATION, authorization)
            .json(request)
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;
        Ok(response.status())
    }

    pub async fn log(
        &self,
        agent: &AgentEndpoint,
        apply_id: &str,
        offset: u64,
    ) -> anyhow::Result<LogChunk> {
        let offset = offset.to_string();
        self.get_json(
            agent,
            "/apply/log",
            &[("apply_id", apply_id), ("offset", &offset)],
        )
        .await
    }

    pub async fn activations(
        &self,
        agent: &AgentEndpoint,
        since: u64,
    ) -> anyhow::Result<ActivationsResponse> {
        let since = since.to_string();
        self.get_json(agent, "/activations", &[("since", &since)])
            .await
    }
}
