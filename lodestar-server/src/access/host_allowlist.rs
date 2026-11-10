//! The tailnet listener's DNS-rebinding guard: the request's `Host` (and absolute-form or
//! HTTP/2 authority) must name this server: its listen address, `localhost` when it listens
//! on loopback, or a configured `allowed_hosts` name. Names are never resolved.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::authority::{Authority, AuthorityHost, single_header};

/// The host names and addresses the tailnet listener answers to.
#[derive(Clone, Debug)]
pub struct HostAllowlist {
    names: Vec<String>,
    ips: Vec<IpAddr>,
    /// Listening on `0.0.0.0` / `::` (only without access control): any IP literal names
    /// this server, as a rebinding page can only ever send its own DNS name.
    any_ip: bool,
}

impl HostAllowlist {
    /// From the listen address and the configured `allowed_hosts`. Entries that are not a
    /// bare host name or IP address are skipped (`ServerConfig::check_allowed_hosts` refuses
    /// them at startup).
    pub fn new(listen: SocketAddr, allowed_hosts: &[String]) -> HostAllowlist {
        let listen_ip = listen.ip().to_canonical();
        let mut allowlist = HostAllowlist {
            names: Vec::new(),
            ips: Vec::new(),
            any_ip: listen_ip.is_unspecified(),
        };
        if listen_ip.is_loopback() || listen_ip.is_unspecified() {
            allowlist.names.push("localhost".into());
        }
        if !listen_ip.is_unspecified() {
            allowlist.ips.push(listen_ip);
        }
        for entry in allowed_hosts {
            match parse_entry(entry) {
                Some(AuthorityHost::Name(name)) => allowlist.names.push(name),
                Some(AuthorityHost::Ip(ip)) => allowlist.ips.push(ip),
                None => {}
            }
        }
        allowlist
    }

    pub fn permits(&self, authority: &Authority) -> bool {
        match &authority.host {
            AuthorityHost::Name(name) => self.names.contains(name),
            AuthorityHost::Ip(ip) => self.any_ip || self.ips.contains(ip),
        }
    }
}

/// An `allowed_hosts` entry: a host name or IP address without a port.
pub fn parse_entry(entry: &str) -> Option<AuthorityHost> {
    let authority = Authority::parse(entry)?;
    authority.port.is_none().then_some(authority.host)
}

/// The authority the request is for: its single `Host` header and the request target's
/// authority (absolute form, HTTP/2), which must agree when both are present. `None` when
/// neither is there, either is malformed, or `Host` is sent several times.
pub fn request_authority(headers: &HeaderMap, uri: &Uri) -> Option<Authority> {
    let from_header = match single_header(headers, &header::HOST)? {
        Some(text) => Some(Authority::parse(text)?),
        None => None,
    };
    let from_uri = match uri.authority() {
        Some(authority) => Some(Authority::parse(authority.as_str())?),
        None => None,
    };
    match (from_header, from_uri) {
        (Some(header), Some(target)) => header.same_as(&target, 80).then_some(header),
        (Some(authority), None) | (None, Some(authority)) => Some(authority),
        (None, None) => None,
    }
}

/// Middleware: plain 403 `forbidden` unless the request names this server.
pub async fn require_known_host(
    State(allowlist): State<Arc<HostAllowlist>>,
    request: Request,
    next: Next,
) -> Response {
    let known = request_authority(request.headers(), request.uri())
        .is_some_and(|authority| allowlist.permits(&authority));
    if known {
        next.run(request).await
    } else {
        let host: String = request
            .headers()
            .get(header::HOST)
            .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
            .unwrap_or_default()
            .chars()
            .take(80)
            .collect();
        eprintln!(
            "lodestar-server: refused a request for host {host:?} (not the listen address or in allowed_hosts)"
        );
        (StatusCode::FORBIDDEN, "forbidden").into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn permits(allowlist: &HostAllowlist, text: &str) -> bool {
        allowlist.permits(&Authority::parse(text).unwrap())
    }

    #[test]
    fn listen_address_and_names() {
        let allowlist = HostAllowlist::new(
            "100.64.0.1:8080".parse().unwrap(),
            &["Manager".into(), "100.64.0.7".into(), "bad:1".into()],
        );
        assert!(permits(&allowlist, "100.64.0.1"));
        assert!(permits(&allowlist, "manager:8080"));
        assert!(permits(&allowlist, "100.64.0.7"));
        assert!(!permits(&allowlist, "localhost"));
        assert!(!permits(&allowlist, "bad"));
        assert!(!permits(&allowlist, "100.64.0.2"));
    }

    #[test]
    fn loopback_and_unspecified() {
        let loopback = HostAllowlist::new("127.0.0.1:8080".parse().unwrap(), &[]);
        assert!(permits(&loopback, "localhost:5173"));
        assert!(permits(&loopback, "127.0.0.1"));
        assert!(!permits(&loopback, "127.0.0.2"));
        let any = HostAllowlist::new("0.0.0.0:8080".parse().unwrap(), &[]);
        assert!(permits(&any, "localhost"));
        assert!(permits(&any, "192.0.2.1"));
        assert!(!permits(&any, "evil.example.com"));
    }
}
