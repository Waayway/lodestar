/// Why a promote did not happen.
#[derive(Debug)]
pub enum PromoteError {
    /// The commit is not a valid promote target (hash, ancestry); nothing was pushed.
    Rejected(String),
    /// git itself failed (rev-parse, merge-base, push).
    Git(anyhow::Error),
}

impl From<anyhow::Error> for PromoteError {
    fn from(error: anyhow::Error) -> Self {
        PromoteError::Git(error)
    }
}

impl std::fmt::Display for PromoteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PromoteError::Rejected(message) => formatter.write_str(message),
            PromoteError::Git(error) => write!(formatter, "{error:#}"),
        }
    }
}

impl std::error::Error for PromoteError {}
