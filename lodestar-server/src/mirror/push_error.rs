/// Why a lock push did not happen.
#[derive(Debug)]
pub enum PushError {
    /// The channel head on the remote is no longer the commit the worktree was made from;
    /// nothing was pushed.
    ChannelMoved { expected: String, actual: String },
    /// The request or the worktree is not a valid lock push (bad channel, base or message,
    /// files other than `flake.lock` changed, or `flake.lock` unchanged); nothing was pushed.
    Rejected(String),
    /// git itself failed.
    Git(anyhow::Error),
}

impl From<anyhow::Error> for PushError {
    fn from(error: anyhow::Error) -> Self {
        PushError::Git(error)
    }
}

impl std::fmt::Display for PushError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PushError::ChannelMoved { expected, actual } => {
                write!(
                    formatter,
                    "channel moved: expected {expected}, found {actual}"
                )
            }
            PushError::Rejected(message) => formatter.write_str(message),
            PushError::Git(error) => write!(formatter, "{error:#}"),
        }
    }
}

impl std::error::Error for PushError {}
