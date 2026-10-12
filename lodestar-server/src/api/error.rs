use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// An error answer: `{"error": "..."}` with the status.
#[derive(Debug)]
pub(super) struct ApiError(pub(super) StatusCode, pub(super) String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

/// Logs `error` and turns it into a 500 answer.
pub(super) fn internal(error: anyhow::Error) -> ApiError {
    eprintln!("lodestar-server: api: {error:#}");
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("{error:#}"))
}
