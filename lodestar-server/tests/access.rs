use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    extract::connect_info::MockConnectInfo,
    http::{Request, StatusCode},
    middleware,
    routing::get,
};
use lodestar_server::access::{Access, AccessState, decide, require_access};
use lodestar_server::config::AccessConfig;
use lodestar_server::tailscale::{Identity, OnlineProvider, PeerInfo, parse_status, parse_whois};
use tower::ServiceExt;

fn access_config() -> AccessConfig {
    AccessConfig::Tailscale {
        allowed_users: vec!["alice@example.com".into()],
        allowed_tags: vec![
            "tag:nixos".into(),
            "tag:personal".into(),
            "tag:lodestar".into(),
        ],
    }
}

fn whois(name: &str) -> Identity {
    parse_whois(&std::fs::read_to_string(format!("tests/fixtures/{name}")).unwrap()).unwrap()
}

#[test]
fn parse_status_fixture() {
    let peers = parse_status(include_str!("fixtures/status.json")).unwrap();
    assert_eq!(peers.len(), 4);
    let me = &peers["manager"];
    assert!(me.online);
    assert_eq!(me.addr, Some("100.64.0.1".parse::<IpAddr>().unwrap()));
    let laptop = &peers["laptop"];
    assert_eq!(laptop.host, "laptop");
    assert!(laptop.online);
    assert_eq!(laptop.os, "macOS");
    assert_eq!(laptop.addr, Some("100.64.0.2".parse::<IpAddr>().unwrap()));
    assert_eq!(laptop.last_seen, Some(1_791_792_000));
    let desktop = &peers["desktop"];
    assert!(!desktop.online);
    assert_eq!(desktop.last_seen, Some(1_791_750_600));
    let ghost = &peers["ghost"];
    assert_eq!(ghost.last_seen, None);
    assert_eq!(
        ghost.addr,
        Some("fd7a:115c:a1e0::4".parse::<IpAddr>().unwrap())
    );
}

#[test]
fn user_login_allowed() {
    let identity = whois("whois-user.json");
    assert_eq!(identity.login.as_deref(), Some("alice@example.com"));
    assert_eq!(identity.node, "laptop");
    assert_eq!(
        decide(&access_config(), Some(&identity)),
        Access::Allowed {
            actor: "alice@example.com".into()
        }
    );
}

#[test]
fn tagged_nixos_allowed() {
    let identity = whois("whois-tagged.json");
    assert_eq!(identity.login, None);
    assert_eq!(identity.tags, vec!["tag:nixos".to_string()]);
    assert_eq!(
        decide(&access_config(), Some(&identity)),
        Access::Allowed {
            actor: "nixos-box".into()
        }
    );
}

#[test]
fn tagged_other_denied() {
    let identity = Identity {
        login: None,
        tags: vec!["tag:server".into()],
        node: "web".into(),
    };
    assert_eq!(decide(&access_config(), Some(&identity)), Access::Denied);
    let stranger = Identity {
        login: Some("eve@example.com".into()),
        tags: vec![],
        node: "x".into(),
    };
    assert_eq!(decide(&access_config(), Some(&stranger)), Access::Denied);
}

#[test]
fn no_identity_denied() {
    assert_eq!(decide(&access_config(), None), Access::Denied);
}

#[test]
fn disabled_allows() {
    assert_eq!(
        decide(&AccessConfig::Disabled, None),
        Access::Allowed {
            actor: "dev".into()
        }
    );
}

struct Fake(Option<Identity>);

#[async_trait::async_trait]
impl OnlineProvider for Fake {
    async fn peers(&self) -> HashMap<String, PeerInfo> {
        HashMap::new()
    }
    async fn whois(&self, _ip: IpAddr) -> Option<Identity> {
        self.0.clone()
    }
}

fn app(provider: Fake, access: AccessConfig) -> Router {
    let provider: Arc<dyn OnlineProvider> = Arc::new(provider);
    Router::new()
        .route("/api/snapshot", get(|| async { "ok" }))
        .route("/api/ws", get(|| async { "ok" }))
        .layer(middleware::from_fn_with_state(
            AccessState {
                provider,
                config: Arc::new(access),
            },
            require_access,
        ))
        .layer(MockConnectInfo(SocketAddr::from(([100, 64, 0, 9], 1234))))
}

async fn status(app: Router, path: &str) -> StatusCode {
    app.oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn middleware_403() {
    let foreign = Identity {
        login: Some("eve@example.com".into()),
        tags: vec![],
        node: "evil".into(),
    };
    for path in ["/api/snapshot", "/api/ws"] {
        assert_eq!(
            status(app(Fake(Some(foreign.clone())), access_config()), path).await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            status(app(Fake(None), access_config()), path).await,
            StatusCode::FORBIDDEN
        );
    }
    let owner = whois("whois-user.json");
    assert_eq!(
        status(app(Fake(Some(owner)), access_config()), "/api/snapshot").await,
        StatusCode::OK
    );
    assert_eq!(
        status(app(Fake(None), AccessConfig::Disabled), "/api/ws").await,
        StatusCode::OK
    );
}

fn peer_json(key: &str, dns_name: &str, ip: &str, online: bool, last_seen: &str) -> String {
    format!(
        r#""{key}":{{"HostName":"Box","DNSName":"{dns_name}","OS":"linux","TailscaleIPs":["{ip}"],"Online":{online},"LastSeen":"{last_seen}"}}"#
    )
}

#[test]
fn duplicate_hostname_prefers_online_regardless_of_order() {
    let off = peer_json(
        "k1",
        "box-old.ts.net.",
        "100.64.0.50",
        false,
        "2026-10-12T09:00:00Z",
    );
    let on = peer_json(
        "k2",
        "box.ts.net.",
        "100.64.0.51",
        true,
        "2026-10-01T00:00:00Z",
    );
    for (first_peer, second_peer) in [(&off, &on), (&on, &off)] {
        let json = format!(r#"{{"Peer":{{{first_peer},{second_peer}}}}}"#);
        let peers = parse_status(&json).unwrap();
        assert_eq!(peers.len(), 1);
        assert!(peers["box"].online);
        assert_eq!(
            peers["box"].addr,
            Some("100.64.0.51".parse::<IpAddr>().unwrap())
        );
    }
}

#[test]
fn duplicate_hostname_tiebreaks_on_last_seen_then_dnsname() {
    let first_peer = peer_json(
        "k1",
        "b.ts.net.",
        "100.64.0.60",
        false,
        "2026-10-01T00:00:00Z",
    );
    let second_peer = peer_json(
        "k2",
        "a.ts.net.",
        "100.64.0.61",
        false,
        "2026-10-01T00:00:00Z",
    );
    let third_peer = peer_json(
        "k3",
        "z.ts.net.",
        "100.64.0.62",
        false,
        "2026-10-05T00:00:00Z",
    );
    let peers = parse_status(&format!(r#"{{"Peer":{{{first_peer},{second_peer}}}}}"#)).unwrap();
    assert_eq!(
        peers["box"].addr,
        Some("100.64.0.61".parse::<IpAddr>().unwrap())
    );
    let peers = parse_status(&format!(
        r#"{{"Peer":{{{first_peer},{second_peer},{third_peer}}}}}"#
    ))
    .unwrap();
    assert_eq!(
        peers["box"].addr,
        Some("100.64.0.62".parse::<IpAddr>().unwrap())
    );
}

#[test]
fn self_wins_over_colliding_peer() {
    let me = r#""Self":{"HostName":"box","DNSName":"zzz.ts.net.","OS":"linux","TailscaleIPs":["100.64.0.1"],"Online":false,"LastSeen":"0001-01-01T00:00:00Z"}"#;
    let other = peer_json(
        "k1",
        "aaa.ts.net.",
        "100.64.0.70",
        true,
        "2026-10-12T09:00:00Z",
    );
    let peers = parse_status(&format!(r#"{{{me},"Peer":{{{other}}}}}"#)).unwrap();
    assert_eq!(
        peers["box"].addr,
        Some("100.64.0.1".parse::<IpAddr>().unwrap())
    );
}
