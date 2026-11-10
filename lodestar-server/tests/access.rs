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
use lodestar_server::tailscale::{Identity, OnlineProvider, PeerInfo, parse_whois};
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
