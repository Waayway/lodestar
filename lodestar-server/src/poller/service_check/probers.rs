use std::error::Error as _;
use std::io;
use std::time::Instant;

use lodestar_proto::{DownReason, ServiceState};
use reqwest::redirect::Policy;

use super::{CHECK_TIMEOUT, ProbeOutcome};
use crate::config::Link;

/// The HTTP clients used for probing: redirects are never followed (a redirect to a login
/// page is a sign of life, and following one could reach anywhere).
pub struct Probers {
    normal: reqwest::Client,
    insecure: reqwest::Client,
}

impl Probers {
    pub fn new() -> anyhow::Result<Self> {
        let base = || {
            reqwest::Client::builder()
                .redirect(Policy::none())
                .timeout(CHECK_TIMEOUT)
        };
        Ok(Self {
            normal: base().build()?,
            insecure: base().danger_accept_invalid_certs(true).build()?,
        })
    }
}

/// GET the link's check URL (or its own URL); latency is measured to the response headers.
pub async fn probe(probers: &Probers, link: &Link) -> ProbeOutcome {
    let client = if link.check.insecure_tls {
        &probers.insecure
    } else {
        &probers.normal
    };
    let url = link.check.url.as_deref().unwrap_or(&link.url);
    let started = Instant::now();
    match client.get(url).send().await {
        Ok(response) => {
            let latency_ms = u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);
            let status = response.status().as_u16();
            if link.check.expect_status.contains(&status) {
                ProbeOutcome {
                    state: ServiceState::Up,
                    reason: None,
                    latency_ms: Some(latency_ms),
                }
            } else {
                ProbeOutcome {
                    state: ServiceState::Down,
                    reason: Some(DownReason::HttpStatus(status)),
                    latency_ms: None,
                }
            }
        }
        Err(error) => ProbeOutcome {
            state: ServiceState::Down,
            reason: Some(classify_error(&error)),
            latency_ms: None,
        },
    }
}

/// Map a failed request to the reason shown on the dashboard.
pub fn classify_error(error: &reqwest::Error) -> DownReason {
    if error.is_timeout() {
        return DownReason::Timeout;
    }
    let mut refused = false;
    let mut tls = false;
    let mut source = error.source();
    while let Some(cause) = source {
        if let Some(io_error) = cause.downcast_ref::<io::Error>() {
            refused |= io_error.kind() == io::ErrorKind::ConnectionRefused;
        }
        tls |= looks_like_tls(&cause.to_string());
        source = cause.source();
    }
    if refused {
        DownReason::ConnectionRefused
    } else if tls {
        DownReason::Tls
    } else if error.is_connect() {
        DownReason::Unreachable
    } else {
        DownReason::Other(error.to_string())
    }
}

/// rustls is not a direct dependency, so its errors are recognised by message: reqwest
/// surfaces them as an `io::Error` wrapping the rustls error text.
fn looks_like_tls(message: &str) -> bool {
    let message = message.to_lowercase();
    [
        "certificate",
        "invalid peer",
        "tls",
        "handshake",
        "unknownissuer",
    ]
    .iter()
    .any(|marker| message.contains(marker))
}
