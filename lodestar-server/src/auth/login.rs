//! `GET /auth/login?next=`: starts a login at the issuer.

use std::sync::Arc;

use axum::Extension;
use axum::extract::Query;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use super::cookie::{LOGIN_COOKIE, LOGIN_TTL_SECS, set_cookie};
use super::error_page::{issuer_unreachable_page, login_failed_page};
use super::{AuthState, LoginError, safe_next};
use crate::clock;

#[derive(Deserialize)]
pub struct LoginQuery {
    next: Option<String>,
}

/// 303 to the issuer's authorize URL, binding the browser to the login with the login cookie
/// (its value is the `state`).
pub async fn login(
    Extension(auth): Extension<Arc<AuthState>>,
    query: Result<Query<LoginQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let next = safe_next(
        query
            .as_ref()
            .ok()
            .and_then(|Query(query)| query.next.as_deref()),
    );
    let now = clock::now();
    let start = match auth.oidc.begin(next, now).await {
        Ok(start) => start,
        Err(LoginError::IssuerUnreachable(reason)) => {
            eprintln!("lodestar-server: login: issuer unreachable: {reason}");
            return issuer_unreachable_page();
        }
        Err(error) => {
            eprintln!("lodestar-server: login: {error}");
            return login_failed_page(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };
    let cookie = set_cookie(LOGIN_COOKIE, &start.state, LOGIN_TTL_SECS);
    auth.pending.insert(start.state, start.pending, now);
    (
        StatusCode::SEE_OTHER,
        [
            (header::LOCATION, start.authorize_url),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        [(header::SET_COOKIE, cookie)],
    )
        .into_response()
}
