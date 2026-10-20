use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

const SESSION_ID_BYTES: usize = 32;
const SESSION_ID_CHARS: usize = 43;

/// The secret cookie value of a session: 32 random bytes, base64url without padding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionId(String);

impl SessionId {
    pub fn new_random() -> SessionId {
        let mut bytes = [0u8; SESSION_ID_BYTES];
        getrandom::fill(&mut bytes).expect("operating system randomness unavailable");
        SessionId(URL_SAFE_NO_PAD.encode(bytes))
    }

    /// Accepts only exactly what `new_random` produces: 43 base64url characters.
    pub fn from_cookie(value: &str) -> Option<SessionId> {
        let valid = value.len() == SESSION_ID_CHARS
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
        valid.then(|| SessionId(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// What the `sessions` table stores instead of the id.
    pub fn hash(&self) -> String {
        hash_session_value(&self.0)
    }
}

/// Lowercase hex sha256.
pub fn hash_session_value(value: &str) -> String {
    Sha256::digest(value.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_sha256_hex() {
        assert_eq!(
            hash_session_value("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn random_ids_are_43_chars_and_differ() {
        let first = SessionId::new_random();
        let second = SessionId::new_random();
        assert_eq!(first.as_str().len(), 43);
        assert_ne!(first.as_str(), second.as_str());
        assert_eq!(first.hash(), hash_session_value(first.as_str()));
        assert!(SessionId::from_cookie(first.as_str()).is_some());
    }

    #[test]
    fn from_cookie_rejects() {
        assert!(SessionId::from_cookie("").is_none());
        assert!(SessionId::from_cookie(&"a".repeat(42)).is_none());
        assert!(SessionId::from_cookie(&"a".repeat(44)).is_none());
        assert!(SessionId::from_cookie(&format!("{}+", "a".repeat(42))).is_none());
        assert!(SessionId::from_cookie(&format!("{}/", "a".repeat(42))).is_none());
    }
}
