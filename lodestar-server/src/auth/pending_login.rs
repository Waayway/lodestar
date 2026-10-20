/// What `/auth/login` remembers until the callback: secrets of one authorization request.
#[derive(Clone)]
pub struct PendingLogin {
    pub nonce: String,
    pub pkce_verifier: String,
    /// Where to go after login (already checked by `safe_next`).
    pub next: String,
    /// Unix seconds.
    pub created_at: i64,
}

impl PendingLogin {
    /// Older than the login cookie (`LOGIN_TTL_SECS`) at `now`.
    pub fn is_expired(&self, now: i64) -> bool {
        now - self.created_at >= super::cookie::LOGIN_TTL_SECS
    }
}
