use std::fmt;

/// Why a login could not start or finish. The texts never contain token contents.
#[derive(Clone, Debug, PartialEq)]
pub enum LoginError {
    /// Discovery, JWKS or token endpoint could not be reached.
    IssuerUnreachable(String),
    /// Our side is wrong: secret file unreadable, issuer metadata unusable.
    Misconfigured(String),
    /// The callback's `state` matches no pending login (replayed, expired, restarted).
    UnknownState,
    /// The issuer or the ID token said no: bad code, verifier, signature, nonce, claims.
    Refused(String),
}

impl fmt::Display for LoginError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoginError::IssuerUnreachable(reason) => {
                write!(formatter, "issuer unreachable: {reason}")
            }
            LoginError::Misconfigured(reason) => write!(formatter, "misconfigured: {reason}"),
            LoginError::UnknownState => write!(formatter, "unknown or expired login state"),
            LoginError::Refused(reason) => write!(formatter, "refused: {reason}"),
        }
    }
}

impl std::error::Error for LoginError {}
