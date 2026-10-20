//! `GET /auth/callback?code=&state=` (or `?error=&state=` from the issuer): finishes a login.

use std::sync::Arc;

use axum::Extension;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use lodestar_proto::Role;
use serde::Deserialize;

use super::cookie::{
    LOGIN_COOKIE, SESSION_COOKIE, SESSION_TTL_SECS, clear_cookie, read_cookie, set_cookie,
};
use super::error_page::{issuer_unreachable_page, login_failed_page, no_access_page};
use super::{AuthState, LoginError, LoginSession, SessionId};
use crate::clock;
use crate::db;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

/// On success: a new session (replacing the one the browser's session cookie names, if any),
/// 303 to the login's `next` with the session cookie set. Every answer clears the login
/// cookie; no failure creates or ends a session.
pub async fn callback(
    State(state): State<AppState>,
    Extension(auth): Extension<Arc<AuthState>>,
    headers: HeaderMap,
    query: Result<Query<CallbackQuery>, QueryRejection>,
) -> Response {
    let mut response = finish_login(&state, &auth, &headers, query).await;
    response
        .headers_mut()
        .append(header::SET_COOKIE, clear_cookie(LOGIN_COOKIE));
    response
}

async fn finish_login(
    state: &AppState,
    auth: &AuthState,
    headers: &HeaderMap,
    query: Result<Query<CallbackQuery>, QueryRejection>,
) -> Response {
    let Ok(Query(query)) = query else {
        return refused("unreadable callback parameters");
    };
    let Some(login_state) = query.state.as_deref() else {
        return refused("callback without state");
    };
    if read_cookie(headers, LOGIN_COOKIE).as_deref() != Some(login_state) {
        return refused("state does not match the login cookie");
    }
    let now = clock::now();
    let pending = auth.pending.take(login_state, now);
    if query.error.is_some() {
        // The issuer's error text is not logged: it is whatever the URL says.
        eprintln!("lodestar-server: login refused: the login service returned an error");
        return no_access_page();
    }
    let Some(code) = query.code.as_deref() else {
        return refused("callback without code");
    };
    let Some(pending) = pending else {
        return refused(&LoginError::UnknownState.to_string());
    };
    let next = pending.next.clone();
    let identity = match auth.oidc.finish(code, pending).await {
        Ok(identity) => identity,
        Err(LoginError::IssuerUnreachable(reason)) => {
            eprintln!("lodestar-server: login refused: issuer unreachable: {reason}");
            return issuer_unreachable_page();
        }
        Err(LoginError::Misconfigured(reason)) => {
            eprintln!("lodestar-server: login refused: misconfigured: {reason}");
            return login_failed_page(StatusCode::INTERNAL_SERVER_ERROR);
        }
        Err(LoginError::UnknownState) => {
            return refused(&LoginError::UnknownState.to_string());
        }
        Err(LoginError::Refused(reason)) => {
            // Only the part before the first `:` ("ID token", "token endpoint"): the rest
            // may quote claims or the issuer's answer.
            let kind = reason.split(':').next().unwrap_or_default();
            return refused(&format!("rejected ({kind})"));
        }
    };
    let Some(role) = identity.role(&auth.config.oidc) else {
        eprintln!(
            "lodestar-server: login refused: {} is in no Lodestar group",
            identity.user
        );
        return no_access_page();
    };
    let session_id = SessionId::new_random();
    let session = LoginSession {
        id_hash: session_id.hash(),
        user: identity.user,
        role,
        expires_at: now + SESSION_TTL_SECS,
    };
    // The session this browser had until now ends, so a copy of its old cookie stops working.
    let replaced = read_cookie(headers, SESSION_COOKIE)
        .and_then(|value| SessionId::from_cookie(&value))
        .map(|id| id.hash());
    if let Err(error) = store_session(state, &session, replaced.as_deref(), now).await {
        eprintln!("lodestar-server: login: storing the session: {error:#}");
        return login_failed_page(StatusCode::INTERNAL_SERVER_ERROR);
    }
    eprintln!(
        "lodestar-server: login: {} as {}",
        session.user,
        role_name(role)
    );
    (
        StatusCode::SEE_OTHER,
        [
            (header::LOCATION, next),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        [(
            header::SET_COOKIE,
            set_cookie(SESSION_COOKIE, session_id.as_str(), SESSION_TTL_SECS),
        )],
    )
        .into_response()
}

/// Logs `reason` and answers 400 `Login failed`.
fn refused(reason: &str) -> Response {
    eprintln!("lodestar-server: login refused: {reason}");
    login_failed_page(StatusCode::BAD_REQUEST)
}

async fn store_session(
    state: &AppState,
    session: &LoginSession,
    replaced: Option<&str>,
    now: i64,
) -> anyhow::Result<()> {
    if let Some(id_hash) = replaced {
        db::delete_session(&state.db, id_hash).await?;
    }
    db::delete_expired_sessions(&state.db, now).await?;
    db::insert_session(&state.db, session, now).await
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::Anonymous => "anonymous",
        Role::Reader => "reader",
        Role::Admin => "admin",
    }
}
