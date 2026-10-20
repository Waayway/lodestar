use axum::http::StatusCode;
use lodestar_proto::{ActDenied, Me, Role};

use crate::auth::LoginSession;

/// What a request may do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    /// Anyone, logged in or not.
    Public,
    /// A logged-in reader or admin.
    Read,
    /// An admin whose request arrived over Tailscale.
    Act,
}

/// Who is asking, as decided by the entrance the request came through.
#[derive(Clone, Debug, PartialEq)]
pub struct Viewer {
    pub name: Option<String>,
    pub role: Role,
    /// Hash of the session cookie; `None` when there is no session.
    pub session_hash: Option<String>,
    pub can_act: bool,
    pub reason: Option<ActDenied>,
}

impl Viewer {
    /// Someone the tailnet listener let in: an admin who can act.
    pub fn tailnet(actor: String) -> Viewer {
        Viewer {
            name: Some(actor),
            role: Role::Admin,
            session_hash: None,
            can_act: true,
            reason: None,
        }
    }

    pub fn anonymous() -> Viewer {
        Viewer {
            name: None,
            role: Role::Anonymous,
            session_hash: None,
            can_act: false,
            reason: Some(ActDenied::Anonymous),
        }
    }

    /// A logged-in viewer; `via_tailnet` is whether the router marked the request.
    pub fn from_session(session: &LoginSession, via_tailnet: bool) -> Viewer {
        let reason = match session.role {
            Role::Anonymous => Some(ActDenied::Anonymous),
            Role::Reader => Some(ActDenied::NotAdmin),
            Role::Admin if !via_tailnet => Some(ActDenied::NotTailnet),
            Role::Admin => None,
        };
        Viewer {
            name: Some(session.user.clone()),
            role: session.role,
            session_hash: Some(session.id_hash.clone()),
            can_act: reason.is_none(),
            reason,
        }
    }

    pub fn me(&self) -> Me {
        Me {
            viewer: self.name.clone(),
            role: self.role,
            session: self.session_hash.is_some(),
            can_act: self.can_act,
            reason: self.reason.clone(),
        }
    }

    /// `Ok` when the viewer meets `need`, else the status and message to refuse with.
    pub fn permits(&self, need: Need) -> Result<(), (StatusCode, &'static str)> {
        if need == Need::Public {
            return Ok(());
        }
        if self.role == Role::Anonymous {
            return Err((StatusCode::UNAUTHORIZED, "log in first"));
        }
        if need == Need::Act {
            match self.reason {
                Some(ActDenied::NotAdmin) => {
                    return Err((StatusCode::FORBIDDEN, "only admins can do this"));
                }
                Some(ActDenied::NotTailnet) => {
                    return Err((StatusCode::FORBIDDEN, "only over Tailscale"));
                }
                Some(ActDenied::Anonymous) => {
                    return Err((StatusCode::UNAUTHORIZED, "log in first"));
                }
                None => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(user: &str, role: Role) -> LoginSession {
        LoginSession {
            id_hash: "hash".into(),
            user: user.into(),
            role,
            expires_at: 0,
        }
    }

    const LOG_IN: (StatusCode, &str) = (StatusCode::UNAUTHORIZED, "log in first");
    const NOT_ADMIN: (StatusCode, &str) = (StatusCode::FORBIDDEN, "only admins can do this");
    const NOT_TAILNET: (StatusCode, &str) = (StatusCode::FORBIDDEN, "only over Tailscale");

    type Refusal = Result<(), (StatusCode, &'static str)>;

    #[test]
    fn permits_table() {
        let rows: Vec<(&str, Viewer, [Refusal; 3])> = vec![
            (
                "anonymous",
                Viewer::anonymous(),
                [Ok(()), Err(LOG_IN), Err(LOG_IN)],
            ),
            (
                "reader marked",
                Viewer::from_session(&session("alice", Role::Reader), true),
                [Ok(()), Ok(()), Err(NOT_ADMIN)],
            ),
            (
                "admin unmarked",
                Viewer::from_session(&session("bob", Role::Admin), false),
                [Ok(()), Ok(()), Err(NOT_TAILNET)],
            ),
            (
                "admin marked",
                Viewer::from_session(&session("bob", Role::Admin), true),
                [Ok(()), Ok(()), Ok(())],
            ),
            (
                "tailnet",
                Viewer::tailnet("dev".into()),
                [Ok(()), Ok(()), Ok(())],
            ),
        ];
        for (label, viewer, expected) in rows {
            for (need, want) in [Need::Public, Need::Read, Need::Act]
                .into_iter()
                .zip(expected)
            {
                assert_eq!(viewer.permits(need), want, "{label} / {need:?}");
            }
        }
    }

    #[test]
    fn me_table() {
        let me = |viewer: Option<&str>, role, session, can_act, reason| Me {
            viewer: viewer.map(String::from),
            role,
            session,
            can_act,
            reason,
        };
        assert_eq!(
            Viewer::anonymous().me(),
            me(
                None,
                Role::Anonymous,
                false,
                false,
                Some(ActDenied::Anonymous)
            )
        );
        assert_eq!(
            Viewer::from_session(&session("alice", Role::Reader), true).me(),
            me(
                Some("alice"),
                Role::Reader,
                true,
                false,
                Some(ActDenied::NotAdmin)
            )
        );
        assert_eq!(
            Viewer::from_session(&session("bob", Role::Admin), false).me(),
            me(
                Some("bob"),
                Role::Admin,
                true,
                false,
                Some(ActDenied::NotTailnet)
            )
        );
        assert_eq!(
            Viewer::from_session(&session("bob", Role::Admin), true).me(),
            me(Some("bob"), Role::Admin, true, true, None)
        );
        assert_eq!(
            Viewer::tailnet("dev".into()).me(),
            me(Some("dev"), Role::Admin, false, true, None)
        );
    }
}
