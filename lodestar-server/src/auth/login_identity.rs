use lodestar_proto::Role;

use crate::config::OidcConfig;

/// Who logged in, as the verified ID token says: `preferred_username` and `groups`.
#[derive(Clone, Debug, PartialEq)]
pub struct LoginIdentity {
    pub user: String,
    pub groups: Vec<String>,
}

impl LoginIdentity {
    /// Admin group → `Admin`, else reader group → `Reader`, else no access. Exact matches.
    pub fn role(&self, config: &OidcConfig) -> Option<Role> {
        let member_of = |group: &str| self.groups.iter().any(|candidate| candidate == group);
        if member_of(&config.admin_group) {
            Some(Role::Admin)
        } else if member_of(&config.reader_group) {
            Some(Role::Reader)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn config() -> OidcConfig {
        OidcConfig {
            issuer: "https://id.example.com/oauth2/openid/lodestar".into(),
            client_id: "lodestar".into(),
            client_secret_file: PathBuf::from("/run/secret"),
            reader_group: "lodestar-users@example.com".into(),
            admin_group: "admins@example.com".into(),
        }
    }

    fn role_of(groups: &[&str]) -> Option<Role> {
        LoginIdentity {
            user: "alice".into(),
            groups: groups.iter().map(|group| group.to_string()).collect(),
        }
        .role(&config())
    }

    #[test]
    fn role_table() {
        assert_eq!(role_of(&["admins@example.com"]), Some(Role::Admin));
        assert_eq!(
            role_of(&["lodestar-users@example.com", "admins@example.com"]),
            Some(Role::Admin)
        );
        assert_eq!(role_of(&["lodestar-users@example.com"]), Some(Role::Reader));
        assert_eq!(role_of(&[]), None);
        assert_eq!(role_of(&["admins@example.com.evil"]), None);
        assert_eq!(role_of(&["LODESTAR-USERS@example.com"]), None);
    }
}
