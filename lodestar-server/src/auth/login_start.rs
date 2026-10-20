use super::PendingLogin;

/// Where to send the browser, and what to remember under `state` until the callback.
pub struct LoginStart {
    pub authorize_url: String,
    pub state: String,
    pub pending: PendingLogin,
}
