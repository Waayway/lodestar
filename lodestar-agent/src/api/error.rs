use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// Logs `error` and answers 500.
pub(super) fn internal<E: std::fmt::Display>(error: E) -> Response {
    eprintln!("lodestar-agent: {error}");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}
