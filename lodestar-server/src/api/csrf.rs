use axum::extract::Request;
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::error::ApiError;

/// The header the dashboard sends on every request. A cross-site form cannot set it, and a
/// cross-site `fetch` cannot set it without a CORS preflight, which this server never allows.
pub const LODESTAR_HEADER: &str = "x-lodestar";

/// CSRF guard: a state-changing request (anything but GET/HEAD/OPTIONS) needs `X-Lodestar: 1`
/// and, when the browser says where it comes from, `Sec-Fetch-Site: same-origin` or `none`.
/// Otherwise 403.
pub(super) async fn require_same_site(request: Request, next: Next) -> Response {
    if matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) {
        return next.run(request).await;
    }
    let headers = request.headers();
    let header_ok = headers
        .get(LODESTAR_HEADER)
        .is_some_and(|value| value.as_bytes() == b"1");
    let site_ok = headers
        .get("sec-fetch-site")
        .is_none_or(|value| matches!(value.as_bytes(), b"same-origin" | b"none"));
    if header_ok && site_ok {
        next.run(request).await
    } else {
        ApiError(
            StatusCode::FORBIDDEN,
            "cross-site or non-dashboard request refused (needs X-Lodestar: 1)".into(),
        )
        .into_response()
    }
}
