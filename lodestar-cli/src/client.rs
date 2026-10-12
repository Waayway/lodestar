use std::time::Duration;

use anyhow::{Context, Result};
use lodestar_proto::Snapshot;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::request_error::RequestError;

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

    async fn send(&self, request: reqwest::RequestBuilder) -> Result<reqwest::Response> {
        let response = request.send().await.map_err(|error| {
            anyhow::Error::new(RequestError::Unreachable(format!(
                "cannot reach the Lodestar server at {} ({error}); is Tailscale up?",
                self.base
            )))
        })?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let text = response.text().await.unwrap_or_default();
        let message = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|json| json["error"].as_str().map(String::from))
            .unwrap_or(text);
        Err(RequestError::Status(status.as_u16(), message).into())
    }

    pub(crate) async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let response = self
            .send(self.http.get(format!("{}{path}", self.base)))
            .await?;
        let body = response.bytes().await.map_err(|error| {
            anyhow::Error::new(RequestError::Unreachable(format!(
                "reading the response of GET {path}: {error}"
            )))
        })?;
        serde_json::from_slice(&body).with_context(|| format!("decoding response of GET {path}"))
    }

    pub(crate) async fn post(&self, path: &str, body: Value) -> Result<Value> {
        let response = self
            .send(self.http.post(format!("{}{path}", self.base)).json(&body))
            .await?;
        response
            .json()
            .await
            .with_context(|| format!("decoding response of POST {path}"))
    }

    pub async fn snapshot(&self) -> Result<Snapshot> {
        self.get("/api/snapshot").await
    }

    pub(crate) async fn log_text(&self, host: &str, id: &str) -> Result<String> {
        let response = self
            .send(
                self.http
                    .get(format!("{}/api/hosts/{host}/applies/{id}/log", self.base)),
            )
            .await?;
        Ok(response.text().await?)
    }
}
