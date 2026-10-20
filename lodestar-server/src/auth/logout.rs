//! `POST /auth/logout` (behind the CSRF guard): ends the session.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};

use super::SessionId;
use super::cookie::{SESSION_COOKIE, clear_cookie, read_cookie};
use crate::db;
use crate::state::AppState;

/// Deletes the session row (if any) and clears the cookie: 204.
pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let session_id =
        read_cookie(&headers, SESSION_COOKIE).and_then(|value| SessionId::from_cookie(&value));
    if let Some(id) = session_id
        && let Err(error) = db::delete_session(&state.db, &id.hash()).await
    {
        eprintln!("lodestar-server: logout: {error:#}");
        return (StatusCode::INTERNAL_SERVER_ERROR, "logout failed").into_response();
    }
    (
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, clear_cookie(SESSION_COOKIE))],
    )
        .into_response()
}
