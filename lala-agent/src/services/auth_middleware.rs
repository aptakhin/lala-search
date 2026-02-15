// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

//! Authentication middleware helpers for Axum.
//!
//! Provides helper functions and types for authentication:
//! - `SESSION_COOKIE_NAME`: The cookie name for sessions
//! - `extract_session_token`: Get the session token from cookies
//! - `get_tenant_from_header`: Get tenant ID from X-Tenant-Id header
//! - `AuthError`: Error type for auth failures

use crate::models::auth::AuthUser;
use axum::http::{header::HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use tower_cookies::{Cookie, Cookies};

/// Cookie name for the session.
pub const SESSION_COOKIE_NAME: &str = "lala_session";

/// Auth error responses.
#[derive(Debug)]
pub enum AuthError {
    MissingSession,
    InvalidSession,
    InsufficientPermissions,
    InternalError(String),
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AuthError::MissingSession => (StatusCode::UNAUTHORIZED, "Authentication required"),
            AuthError::InvalidSession => (StatusCode::UNAUTHORIZED, "Invalid or expired session"),
            AuthError::InsufficientPermissions => {
                (StatusCode::FORBIDDEN, "Insufficient permissions")
            }
            AuthError::InternalError(msg) => {
                eprintln!("Auth internal error: {}", msg);
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
            }
        };
        (status, message).into_response()
    }
}

/// Extract the session token from cookies.
pub fn extract_session_token(cookies: &Cookies) -> Option<String> {
    cookies
        .get(SESSION_COOKIE_NAME)
        .map(|c| c.value().to_string())
}

/// Get the tenant ID from the X-Tenant-Id header.
/// Falls back to the session's tenant if not provided.
pub fn get_tenant_from_header(headers: &HeaderMap, auth_user: &AuthUser) -> String {
    headers
        .get("X-Tenant-Id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| auth_user.tenant_id.clone())
}

/// Create a session cookie with the given token.
pub fn create_session_cookie(token: &str, max_age_days: u64) -> Cookie<'static> {
    let max_age_secs = max_age_days * 24 * 60 * 60;
    Cookie::build((SESSION_COOKIE_NAME, token.to_string()))
        .path("/")
        .http_only(true)
        .secure(true)
        .same_site(tower_cookies::cookie::SameSite::Lax)
        .max_age(tower_cookies::cookie::time::Duration::seconds(
            max_age_secs as i64,
        ))
        .build()
}

/// Create a cookie that clears the session (for signout).
pub fn clear_session_cookie() -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE_NAME, ""))
        .path("/")
        .http_only(true)
        .max_age(tower_cookies::cookie::time::Duration::ZERO)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_cookie_name() {
        assert_eq!(SESSION_COOKIE_NAME, "lala_session");
    }

    #[test]
    fn test_auth_error_status_codes() {
        use axum::body::Body;
        use axum::http::Response;

        let response: Response<Body> = AuthError::MissingSession.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let response: Response<Body> = AuthError::InvalidSession.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let response: Response<Body> = AuthError::InsufficientPermissions.into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let response: Response<Body> = AuthError::InternalError("test".to_string()).into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_create_session_cookie() {
        let cookie = create_session_cookie("test_token", 365);
        assert_eq!(cookie.name(), SESSION_COOKIE_NAME);
        assert_eq!(cookie.value(), "test_token");
        assert!(cookie.http_only().unwrap_or(false));
    }

    #[test]
    fn test_clear_session_cookie() {
        let cookie = clear_session_cookie();
        assert_eq!(cookie.name(), SESSION_COOKIE_NAME);
        assert_eq!(cookie.value(), "");
    }
}
