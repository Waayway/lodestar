//! The plain HTML page the login routes answer with when something goes wrong.

use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};

/// A small HTML page with `title`, `detail` and a `Try again` link to `/auth/login`.
pub fn error_page(status: StatusCode, title: &str, detail: &str) -> Response {
    let title = escape_html(title);
    let detail = escape_html(detail);
    let body = format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{title} · Lodestar</title>\n</head>\n<body>\n<h1>{title}</h1>\n<p>{detail}</p>\n\
         <p><a href=\"/auth/login\">Try again</a></p>\n</body>\n</html>\n"
    );
    (status, [(header::CACHE_CONTROL, "no-store")], Html(body)).into_response()
}

/// The issuer could not be reached (502).
pub(super) fn issuer_unreachable_page() -> Response {
    error_page(
        StatusCode::BAD_GATEWAY,
        "Can't reach the login service",
        "Try again in a moment.",
    )
}

/// The user is in neither group, or the issuer said no (403).
pub(super) fn no_access_page() -> Response {
    error_page(
        StatusCode::FORBIDDEN,
        "No access",
        "Your account is not in a group that may use Lodestar.",
    )
}

/// Any other failure: 400 for a bad callback, 500 when our side is wrong.
pub(super) fn login_failed_page(status: StatusCode) -> Response {
    error_page(status, "Login failed", "Start the login again.")
}

/// Text content and double-quoted attribute values; `'` stays as is.
fn escape_html(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            other => escaped.push(other),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_markup() {
        assert_eq!(
            escape_html(r#"<a href="x">Tom & Jerry's</a>"#),
            "&lt;a href=&quot;x&quot;&gt;Tom &amp; Jerry's&lt;/a&gt;"
        );
    }
}
