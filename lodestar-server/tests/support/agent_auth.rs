//! The agent token in tests: one token file per host (mode 0600) and a middleware for fake
//! agents that answers 401 unless the request carries that token, like the real agent.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use axum::extract::Request;
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

pub const AGENT_TOKEN: &str = "3f9c1a7e5b2d4c6f8a0e1b3d5f7a9c2e4b6d8f0a1c3e5b7d9f2a4c6e8b0d1f3a";

/// Writes `AGENT_TOKEN` to `<dir>/agent-tokens/<host>` for every host; the value for
/// `ServerConfig::agent_token_files`.
pub fn agent_token_files<'a>(
    dir: &Path,
    hosts: impl IntoIterator<Item = &'a str>,
) -> BTreeMap<String, PathBuf> {
    let tokens_dir = dir.join("agent-tokens");
    std::fs::create_dir_all(&tokens_dir).unwrap();
    hosts
        .into_iter()
        .map(|host| {
            let path = tokens_dir.join(host);
            std::fs::write(&path, format!("{AGENT_TOKEN}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            (host.to_string(), path)
        })
        .collect()
}

/// Fake-agent middleware: 401 unless `Authorization: Bearer AGENT_TOKEN`.
pub async fn require_agent_token(request: Request, next: Next) -> Response {
    let expected = format!("Bearer {AGENT_TOKEN}");
    let presented = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    if presented == Some(expected.as_str()) {
        next.run(request).await
    } else {
        StatusCode::UNAUTHORIZED.into_response()
    }
}
