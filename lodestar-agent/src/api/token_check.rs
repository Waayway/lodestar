use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use lodestar_proto::{AgentToken, bearer_token};

use super::AppState;

/// Middleware: the request must carry `Authorization: Bearer <token>` with a token from the
/// token file. The file is read on every request, so a rotation needs no restart; when it
/// is unusable (unreadable, a symlink, too open, not in the token format) the agent refuses
/// everything (503) rather than fall back to the source check alone.
pub(super) async fn check_token(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let tokens = match AgentToken::read_file(&state.token_file) {
        Ok(tokens) => tokens,
        Err(error) => {
            eprintln!("lodestar-agent: refusing requests: {error:#}");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };
    let presented = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(bearer_token);
    // Check every token, so the time taken does not depend on which one matched.
    let accepted = presented.is_some_and(|presented| {
        tokens
            .iter()
            .fold(false, |accepted, token| token.matches(presented) | accepted)
    });
    if accepted {
        next.run(request).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Bearer")],
        )
            .into_response()
    }
}
