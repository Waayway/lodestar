//! The two cookies (session and login binding) and their exact attributes.

use axum::http::{HeaderMap, HeaderValue, header};

pub const SESSION_COOKIE: &str = "__Host-lodestar_session";
pub const LOGIN_COOKIE: &str = "__Host-lodestar_login";
pub const SESSION_TTL_SECS: i64 = 43_200;
pub const LOGIN_TTL_SECS: i64 = 600;

/// `value` must be cookie-safe (base64url session ids and login states are).
pub fn set_cookie(name: &str, value: &str, max_age: i64) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "{name}={value}; Path=/; Max-Age={max_age}; Secure; HttpOnly; SameSite=Lax"
    ))
    .expect("cookie value is header-safe")
}

pub fn clear_cookie(name: &str) -> HeaderValue {
    set_cookie(name, "", 0)
}

pub fn read_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|line| line.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue, header};

    #[test]
    fn session_cookie_attributes() {
        assert_eq!(
            set_cookie(SESSION_COOKIE, "v", SESSION_TTL_SECS),
            "__Host-lodestar_session=v; Path=/; Max-Age=43200; Secure; HttpOnly; SameSite=Lax"
        );
        assert_eq!(
            set_cookie(LOGIN_COOKIE, "s", LOGIN_TTL_SECS),
            "__Host-lodestar_login=s; Path=/; Max-Age=600; Secure; HttpOnly; SameSite=Lax"
        );
    }

    #[test]
    fn clear_cookie_has_max_age_0() {
        assert_eq!(
            clear_cookie(SESSION_COOKIE),
            "__Host-lodestar_session=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Lax"
        );
    }

    #[test]
    fn read_cookie_finds_among_several() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("a=1; __Host-lodestar_session=x; b=2"),
        );
        assert_eq!(read_cookie(&headers, SESSION_COOKIE).as_deref(), Some("x"));
        assert_eq!(read_cookie(&headers, "missing"), None);
    }
}
