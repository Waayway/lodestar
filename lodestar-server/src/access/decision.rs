use crate::config::AccessConfig;
use crate::tailscale::Identity;

#[derive(Debug, PartialEq)]
pub enum Access {
    Allowed { actor: String },
    Denied,
}

pub fn decide(config: &AccessConfig, identity: Option<&Identity>) -> Access {
    let (allowed_users, allowed_tags) = match config {
        AccessConfig::Disabled => {
            return Access::Allowed {
                actor: "dev".into(),
            };
        }
        AccessConfig::Tailscale {
            allowed_users,
            allowed_tags,
        } => (allowed_users, allowed_tags),
    };
    let Some(identity) = identity else {
        return Access::Denied;
    };
    let user_ok = identity
        .login
        .as_ref()
        .is_some_and(|login| allowed_users.contains(login));
    let tag_ok = identity.tags.iter().any(|tag| allowed_tags.contains(tag));
    if user_ok || tag_ok {
        Access::Allowed {
            actor: identity
                .login
                .clone()
                .unwrap_or_else(|| identity.node.clone()),
        }
    } else {
        Access::Denied
    }
}
