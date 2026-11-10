//! `host[:port]` as browsers send it in `Host` and `Origin`, parsed exactly.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use axum::http::{HeaderMap, HeaderName};

/// The host part: a DNS name (lowercase, without a trailing dot) or an IP address
/// (canonical: an IPv4-mapped IPv6 address is its IPv4 address).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthorityHost {
    Name(String),
    Ip(IpAddr),
}

/// `host[:port]`: a DNS name, a dotted IPv4 address or a bracketed IPv6 address, with an
/// optional decimal port. Userinfo, paths, whitespace, zone ids, unbracketed IPv6 and empty
/// or out-of-range ports are refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Authority {
    pub host: AuthorityHost,
    pub port: Option<u16>,
}

/// URL scheme of an `Origin`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    Http,
    Https,
}

impl Scheme {
    pub fn default_port(self) -> u16 {
        match self {
            Scheme::Http => 80,
            Scheme::Https => 443,
        }
    }
}

impl Authority {
    /// Parses `host[:port]` (a `Host` header value, or the part of an origin after `://`).
    pub fn parse(text: &str) -> Option<Authority> {
        if let Some(rest) = text.strip_prefix('[') {
            let (inside, after) = rest.split_once(']')?;
            let ip: Ipv6Addr = inside.parse().ok()?;
            let port = match after {
                "" => None,
                _ => Some(parse_port(after.strip_prefix(':')?)?),
            };
            return Some(Authority {
                host: AuthorityHost::Ip(IpAddr::V6(ip).to_canonical()),
                port,
            });
        }
        let (host, port) = match text.split_once(':') {
            Some((host, port)) => (host, Some(parse_port(port)?)),
            None => (text, None),
        };
        Some(Authority {
            host: parse_host(host)?,
            port,
        })
    }

    /// Parses a serialized origin, `scheme://host[:port]` with scheme `http` or `https` and
    /// nothing after the authority (no path, not even `/`). `null` and lists are refused.
    pub fn parse_origin(text: &str) -> Option<(Scheme, Authority)> {
        let (scheme, rest) = text.split_once("://")?;
        let scheme = if scheme.eq_ignore_ascii_case("http") {
            Scheme::Http
        } else if scheme.eq_ignore_ascii_case("https") {
            Scheme::Https
        } else {
            return None;
        };
        Some((scheme, Authority::parse(rest)?))
    }

    /// Same host and, with `default_port` for a missing one, the same port.
    pub fn same_as(&self, other: &Authority, default_port: u16) -> bool {
        self.host == other.host
            && self.port.unwrap_or(default_port) == other.port.unwrap_or(default_port)
    }
}

/// Header `name` when it is sent at most once as visible ASCII: `Some(None)` when absent,
/// `Some(Some(value))` when sent once, `None` (unusable) when sent several times or not text.
pub fn single_header<'a>(headers: &'a HeaderMap, name: &HeaderName) -> Option<Option<&'a str>> {
    let mut values = headers.get_all(name).iter();
    match (values.next(), values.next()) {
        (None, _) => Some(None),
        (Some(value), None) => value.to_str().ok().map(Some),
        (Some(_), Some(_)) => None,
    }
}

fn parse_port(text: &str) -> Option<u16> {
    if text.is_empty() || text.len() > 5 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok().filter(|port| *port != 0)
}

/// A dotted IPv4 address or a DNS name of letters, digits and inner hyphens.
fn parse_host(text: &str) -> Option<AuthorityHost> {
    if let Ok(ip) = text.parse::<Ipv4Addr>() {
        return Some(AuthorityHost::Ip(IpAddr::V4(ip)));
    }
    let name = text.strip_suffix('.').unwrap_or(text).to_ascii_lowercase();
    if name.is_empty() || name.len() > 253 {
        return None;
    }
    let labels_ok = name.split('.').all(|label| {
        (1..=63).contains(&label.len())
            && label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            && !label.starts_with('-')
            && !label.ends_with('-')
    });
    // An all-digit last label is a malformed IPv4 address (`100.64.1`), not a name.
    let last_numeric = name
        .rsplit('.')
        .next()
        .is_some_and(|label| label.bytes().all(|byte| byte.is_ascii_digit()));
    (labels_ok && !last_numeric).then_some(AuthorityHost::Name(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str, port: Option<u16>) -> Option<Authority> {
        Some(Authority {
            host: AuthorityHost::Name(text.into()),
            port,
        })
    }

    fn ip(text: &str, port: Option<u16>) -> Option<Authority> {
        Some(Authority {
            host: AuthorityHost::Ip(text.parse().unwrap()),
            port,
        })
    }

    #[test]
    fn parses_hosts() {
        let rows = [
            ("manager", name("manager", None)),
            ("Manager:8080", name("manager", Some(8080))),
            ("manager.:8080", name("manager", Some(8080))),
            (
                "manager.tail0000.ts.net",
                name("manager.tail0000.ts.net", None),
            ),
            ("100.64.0.1:8080", ip("100.64.0.1", Some(8080))),
            (
                "[fd7a:115c:a1e0::1]:8080",
                ip("fd7a:115c:a1e0::1", Some(8080)),
            ),
            ("[FD7A:115C:A1E0:0::1]", ip("fd7a:115c:a1e0::1", None)),
            ("[::ffff:100.64.0.1]:80", ip("100.64.0.1", Some(80))),
            ("x:65535", name("x", Some(65535))),
        ];
        for (text, want) in rows {
            assert_eq!(Authority::parse(text), want, "{text}");
        }
        for text in [
            "",
            ":8080",
            "manager:",
            "manager:0",
            "manager:65536",
            "manager:080808",
            "manager:+80",
            "manager:8080:8080",
            "alice@manager",
            "manager/",
            "manager 8080",
            " manager",
            "manager..ts.net",
            "-manager",
            "manager-",
            "man_ager",
            "100.64.1",
            "100.64.0.01",
            "fd7a::1",
            "[fd7a::1",
            "[fd7a::1]8080",
            "[fd7a::1%eth0]",
            "[100.64.0.1]",
            "[]",
            "*",
        ] {
            assert_eq!(Authority::parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn parses_origins() {
        assert_eq!(
            Authority::parse_origin("http://manager:8080"),
            Some((Scheme::Http, name("manager", Some(8080)).unwrap()))
        );
        assert_eq!(
            Authority::parse_origin("HTTPS://Lodestar.Example.com"),
            Some((Scheme::Https, name("lodestar.example.com", None).unwrap()))
        );
        for text in [
            "null",
            "",
            "manager:8080",
            "ws://manager",
            "http://manager/",
            "http://manager:8080/api",
            "http://manager?x",
            "http://alice@manager",
            "http://manager http://evil.example.com",
            "http:///manager",
        ] {
            assert_eq!(Authority::parse_origin(text), None, "{text:?}");
        }
    }

    #[test]
    fn default_ports_compare_equal() {
        let bare = Authority::parse("manager").unwrap();
        let explicit = Authority::parse("manager:80").unwrap();
        assert!(bare.same_as(&explicit, 80));
        assert!(!bare.same_as(&explicit, 443));
        assert!(!bare.same_as(&Authority::parse("other").unwrap(), 80));
    }
}
