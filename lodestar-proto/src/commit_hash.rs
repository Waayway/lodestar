/// True for exactly 40 lowercase hex characters.
pub fn is_commit_hash(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_hash_validation() {
        assert!(is_commit_hash(&"a".repeat(40)));
        assert!(!is_commit_hash(&"a".repeat(39)));
        assert!(!is_commit_hash(&"A".repeat(40)));
        assert!(!is_commit_hash(&format!("{}-dirty", "a".repeat(40))));
    }
}
