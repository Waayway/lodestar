const MAX_NEXT_BYTES: usize = 512;

/// The post-login redirect target: a local absolute path, otherwise `/`.
pub fn safe_next(raw: Option<&str>) -> String {
    match raw {
        Some(path)
            if path.starts_with('/')
                && !path.starts_with("//")
                && !path.starts_with("/\\")
                && path.len() <= MAX_NEXT_BYTES
                && !path.chars().any(char::is_control) =>
        {
            path.to_string()
        }
        _ => "/".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_local_paths() {
        assert_eq!(safe_next(Some("/")), "/");
        assert_eq!(safe_next(Some("/#/services")), "/#/services");
    }

    #[test]
    fn falls_back_to_root() {
        let long = format!("/{}", "a".repeat(512));
        for raw in [
            None,
            Some(""),
            Some("//evil.example.com"),
            Some("/\\evil.example.com"),
            Some("https://evil.example.com"),
            Some("services"),
            Some("/a\nb"),
            Some("/a\u{7f}b"),
            Some(long.as_str()),
        ] {
            assert_eq!(safe_next(raw), "/", "{raw:?}");
        }
    }
}
