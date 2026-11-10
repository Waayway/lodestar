use std::fs::OpenOptions;
use std::io::Read;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use anyhow::Context;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Length of 32 bytes as lowercase hex (`openssl rand -hex 32`).
const HEX_LEN: usize = 64;
/// Length of 32 bytes as unpadded base64url.
const BASE64URL_LEN: usize = 43;

/// The bearer token the server sends to an agent (`Authorization: Bearer <token>`) on every
/// request. Read from a secret file at runtime; never printed (`Debug` is redacted and no
/// error message contains it).
#[derive(Clone)]
pub struct AgentToken {
    secret: String,
    digest: [u8; 32],
}

impl AgentToken {
    /// A token in a canonical encoding of 32 bytes: 64 lowercase hex digits
    /// (`openssl rand -hex 32`) or 43 unpadded base64url characters. Only the format is
    /// checked: whether the bytes are random cannot be, so generate them with a CSPRNG.
    pub fn parse(text: &str) -> anyhow::Result<AgentToken> {
        let hex = text.len() == HEX_LEN
            && text
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        let base64url = text.len() == BASE64URL_LEN
            && text
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
        anyhow::ensure!(
            hex || base64url,
            "token must encode 32 random bytes as 64 lowercase hex digits or 43 unpadded \
             base64url characters (got {} characters); generate one with `openssl rand -hex 32`",
            text.len()
        );
        Ok(AgentToken {
            secret: text.to_string(),
            digest: Sha256::digest(text.as_bytes()).into(),
        })
    }

    /// Every token in `path`, one per non-empty line (an agent accepts each of them, so a
    /// rotation can list the old and the new token for a while). The file is opened once
    /// without following a symlink, and its type and mode are checked on that descriptor
    /// before reading from it, so the path cannot be swapped between check and read. Refuses
    /// a symlink, anything but a regular file, a file group or others may access, a file in
    /// a directory group or others may write (unless sticky), an empty file and any token
    /// not in the canonical format.
    pub fn read_file(path: &Path) -> anyhow::Result<Vec<AgentToken>> {
        let parent = match path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent,
            _ => Path::new("."),
        };
        let directory_mode = std::fs::metadata(parent)
            .with_context(|| format!("reading the directory of token file {}", path.display()))?
            .permissions()
            .mode();
        anyhow::ensure!(
            directory_mode & 0o022 == 0 || directory_mode & 0o1000 != 0,
            "token file {} is in a directory group or others may write (mode {:o})",
            path.display(),
            directory_mode & 0o7777
        );
        // O_NONBLOCK: opening a FIFO must not hang; it is refused below.
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .with_context(|| {
                format!(
                    "opening token file {} (a symlink is refused)",
                    path.display()
                )
            })?;
        let metadata = file
            .metadata()
            .with_context(|| format!("reading token file {}", path.display()))?;
        anyhow::ensure!(
            metadata.file_type().is_file(),
            "token file {} is not a regular file",
            path.display()
        );
        anyhow::ensure!(
            metadata.permissions().mode() & 0o077 == 0,
            "token file {} is accessible by group or others (mode {:o}); make it 0400 or 0600",
            path.display(),
            metadata.permissions().mode() & 0o777
        );
        let mut text = String::new();
        file.read_to_string(&mut text)
            .with_context(|| format!("reading token file {}", path.display()))?;
        let tokens = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .enumerate()
            .map(|(index, line)| {
                AgentToken::parse(line)
                    .with_context(|| format!("token file {}, token {}", path.display(), index + 1))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        anyhow::ensure!(
            !tokens.is_empty(),
            "token file {} has no token",
            path.display()
        );
        Ok(tokens)
    }

    /// The single token in `path` (what the server sends); see `read_file`.
    pub fn read_one(path: &Path) -> anyhow::Result<AgentToken> {
        let mut tokens = AgentToken::read_file(path)?;
        anyhow::ensure!(
            tokens.len() == 1,
            "token file {} must hold exactly one token, found {}",
            path.display(),
            tokens.len()
        );
        Ok(tokens.remove(0))
    }

    /// Whether `presented` is this token; compares SHA-256 digests in constant time, so
    /// neither the content nor the length leaks through timing.
    pub fn matches(&self, presented: &str) -> bool {
        let digest: [u8; 32] = Sha256::digest(presented.as_bytes()).into();
        bool::from(self.digest.ct_eq(&digest))
    }

    /// The `Authorization` header value.
    pub fn authorization(&self) -> String {
        format!("Bearer {}", self.secret)
    }
}

impl std::fmt::Debug for AgentToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AgentToken(<redacted>)")
    }
}

/// The token of an `Authorization: Bearer <token>` header value (scheme case-insensitive).
pub fn bearer_token(header: &str) -> Option<&str> {
    let (scheme, token) = header.split_once(' ')?;
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then_some(token)
}
