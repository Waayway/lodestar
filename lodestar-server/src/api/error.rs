use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use uuid::Uuid;

/// An error answer: `{"error": "...", "id": "..."}` with the status (`id` only on a logged
/// internal error).
#[derive(Debug)]
pub(super) struct ApiError(pub(super) StatusCode, pub(super) String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

/// Logs the full error chain (with a short id to find it by) and turns it into a 500 answer
/// that carries only that id. `anyhow`'s `{:#}` context chain can include SQL text, absolute
/// state-directory paths and SeaORM internals, none of which belongs in a response any
/// authenticated reader can request (`/api/audit`, `/api/hosts/{n}/applies/{id}/log`,
/// `/api/snapshot`). The explicitly operator-facing text in `apply_request::refuse` is a
/// separate, intentional channel and is unaffected by this.
pub(super) fn internal(error: anyhow::Error) -> ApiError {
    let id = Uuid::new_v4();
    eprintln!("lodestar-server: api: [{id}] {error:#}");
    ApiError(
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("internal error; see the server log for id {id}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_logs_the_chain_but_answers_with_only_an_id() {
        let error = anyhow::anyhow!("reading /var/lib/lodestar-server/state.sqlite")
            .context("a SQL statement against the secrets table");
        let api_error = internal(error);
        assert_eq!(api_error.0, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!api_error.1.contains("state.sqlite"), "{}", api_error.1);
        assert!(!api_error.1.contains("secrets table"), "{}", api_error.1);
        let id = api_error.1.rsplit(' ').next().unwrap();
        assert!(Uuid::try_parse(id).is_ok(), "{}", api_error.1);
    }
}
