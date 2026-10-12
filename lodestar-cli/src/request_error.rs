/// Why a request to the server failed, for telling a restarting server from a real error.
#[derive(Debug)]
pub enum RequestError {
    /// No answer: connection refused or reset, timeout, or the body broke off.
    Unreachable(String),
    /// The server answered with an error status.
    Status(u16, String),
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RequestError::Unreachable(message) => formatter.write_str(message),
            RequestError::Status(403, message) => write!(
                formatter,
                "not allowed: the server only accepts the owner's tailnet devices ({message})"
            ),
            RequestError::Status(code, message) => {
                write!(formatter, "server answered {code}: {message}")
            }
        }
    }
}

impl std::error::Error for RequestError {}

/// True for failures a server restart causes (no answer, or 502/503/504 from a proxy or a
/// server still starting); false for anything the server decided (403, 404, bad JSON, ...).
pub fn is_transient(error: &anyhow::Error) -> bool {
    match error.downcast_ref::<RequestError>() {
        Some(RequestError::Unreachable(_)) => true,
        Some(RequestError::Status(code, _)) => matches!(code, 502..=504),
        None => false,
    }
}
