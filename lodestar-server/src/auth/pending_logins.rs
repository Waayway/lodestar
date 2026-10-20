use std::collections::HashMap;
use std::sync::Mutex;

use super::PendingLogin;

/// At most this many logins wait for their callback; the oldest is dropped beyond it.
pub const MAX_PENDING_LOGINS: usize = 256;

/// Pending logins by `state`, in memory (a restart forgets them: the callback then fails).
pub struct PendingLogins {
    by_state: Mutex<HashMap<String, PendingLogin>>,
}

impl PendingLogins {
    pub fn new() -> Self {
        PendingLogins {
            by_state: Mutex::new(HashMap::new()),
        }
    }

    /// Prunes expired logins, adds this one, then drops the oldest beyond the cap.
    pub fn insert(&self, state: String, pending: PendingLogin, now: i64) {
        let mut by_state = self.by_state.lock().expect("pending logins lock");
        by_state.retain(|_, waiting| !waiting.is_expired(now));
        by_state.insert(state, pending);
        while by_state.len() > MAX_PENDING_LOGINS {
            let oldest = by_state
                .iter()
                .min_by_key(|(_, waiting)| waiting.created_at)
                .map(|(state, _)| state.clone())
                .expect("over the cap, so not empty");
            by_state.remove(&oldest);
        }
    }

    /// Removes and returns the login for `state`; `None` when unknown or expired.
    pub fn take(&self, state: &str, now: i64) -> Option<PendingLogin> {
        let pending = self
            .by_state
            .lock()
            .expect("pending logins lock")
            .remove(state)?;
        (!pending.is_expired(now)).then_some(pending)
    }
}

impl Default for PendingLogins {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::cookie::LOGIN_TTL_SECS;

    fn pending(created_at: i64) -> PendingLogin {
        PendingLogin {
            nonce: "nonce".into(),
            pkce_verifier: "verifier".into(),
            next: "/".into(),
            created_at,
        }
    }

    #[test]
    fn take_twice_is_none() {
        let logins = PendingLogins::new();
        logins.insert("s1".into(), pending(1000), 1000);
        assert_eq!(
            logins.take("s1", 1001).map(|login| login.created_at),
            Some(1000)
        );
        assert!(logins.take("s1", 1002).is_none());
        assert!(logins.take("unknown", 1002).is_none());
    }

    #[test]
    fn expired_is_none() {
        let logins = PendingLogins::new();
        logins.insert("s1".into(), pending(1000), 1000);
        assert!(logins.take("s1", 1000 + LOGIN_TTL_SECS).is_none());
    }

    #[test]
    fn insert_prunes_expired() {
        let logins = PendingLogins::new();
        logins.insert("old".into(), pending(1000), 1000);
        logins.insert(
            "new".into(),
            pending(1000 + LOGIN_TTL_SECS),
            1000 + LOGIN_TTL_SECS,
        );
        assert_eq!(logins.by_state.lock().unwrap().len(), 1);
    }

    #[test]
    fn capped_at_256_dropping_oldest() {
        let logins = PendingLogins::new();
        for index in 0..=MAX_PENDING_LOGINS as i64 {
            logins.insert(format!("s{index}"), pending(1000 + index), 1000 + index);
        }
        assert_eq!(logins.by_state.lock().unwrap().len(), MAX_PENDING_LOGINS);
        let now = 1000 + MAX_PENDING_LOGINS as i64;
        assert!(logins.take("s0", now).is_none());
        assert!(logins.take("s1", now).is_some());
        assert!(
            logins
                .take(&format!("s{MAX_PENDING_LOGINS}"), now)
                .is_some()
        );
    }
}
