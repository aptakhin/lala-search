// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

//! Authentication route handlers.

use crate::models::auth::{
    AuthUser, InviteUserRequest, InviteUserResponse, ListOrgsResponse, MeResponse, MemberInfo,
    MessageResponse, OrgInfo, OrgMembersResponse, RequestLinkRequest, RequestLinkResponse,
    UserRole, VerifyLinkResponse,
};
use crate::services::auth::{AuthConfig, AuthService, InviteRequest};
use crate::services::auth_db::AuthDbClient;
use crate::services::auth_middleware::{
    clear_session_cookie, create_session_cookie, extract_session_token,
};
use crate::services::email::EmailService;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use std::sync::Arc;
use tower_cookies::Cookies;

/// State for auth routes.
#[derive(Clone)]
pub struct AuthState {
    pub auth_service: Arc<AuthService>,
    pub auth_config: AuthConfig,
    pub default_tenant_id: String,
}

impl AuthState {
    /// Create a new auth state from components.
    pub fn new(
        auth_db: AuthDbClient,
        email_service: EmailService,
        auth_config: AuthConfig,
        default_tenant_id: String,
    ) -> Self {
        let auth_service = Arc::new(AuthService::new(
            auth_db,
            email_service,
            auth_config.clone(),
        ));
        Self {
            auth_service,
            auth_config,
            default_tenant_id,
        }
    }
}

/// Create auth router with all authentication routes.
pub fn auth_router() -> Router<AuthState> {
    Router::new()
        // Public routes (no auth required)
        .route("/request-link", post(request_link_handler))
        .route("/verify/{token}", get(verify_link_handler))
        .route(
            "/invitations/{token}/accept",
            get(accept_invitation_handler),
        )
        // Protected routes (auth required)
        .route("/me", get(me_handler))
        .route("/signout", post(signout_handler))
        .route("/organizations", get(list_organizations_handler))
        .route(
            "/organizations/{tenant_id}/members",
            get(list_members_handler),
        )
        .route(
            "/organizations/{tenant_id}/invite",
            post(invite_user_handler),
        )
        .route(
            "/organizations/{tenant_id}/members/{user_id}",
            axum::routing::delete(remove_member_handler),
        )
}

// ============================================================================
// Helper to extract authenticated user
// ============================================================================

async fn get_auth_user(
    state: &AuthState,
    cookies: &Cookies,
) -> Result<AuthUser, (StatusCode, Json<MessageResponse>)> {
    let session_token = extract_session_token(cookies).ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(MessageResponse {
                success: false,
                message: "No session cookie".to_string(),
            }),
        )
    })?;

    state
        .auth_service
        .validate_session(&session_token)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(MessageResponse {
                    success: false,
                    message: format!("Session validation error: {}", e),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(MessageResponse {
                    success: false,
                    message: "Invalid or expired session".to_string(),
                }),
            )
        })
}

// ============================================================================
// Public Route Handlers
// ============================================================================

/// POST /auth/request-link - Request a magic link email.
async fn request_link_handler(
    State(state): State<AuthState>,
    Json(payload): Json<RequestLinkRequest>,
) -> Result<Json<RequestLinkResponse>, (StatusCode, Json<RequestLinkResponse>)> {
    state
        .auth_service
        .request_magic_link(&payload.email)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(RequestLinkResponse {
                    success: false,
                    message: format!("Failed to send magic link: {}", e),
                }),
            )
        })?;

    Ok(Json(RequestLinkResponse {
        success: true,
        message: "If an account exists for this email, a magic link has been sent.".to_string(),
    }))
}

/// GET /auth/verify/{token} - Verify magic link and create session.
async fn verify_link_handler(
    State(state): State<AuthState>,
    cookies: Cookies,
    Path(token): Path<String>,
) -> Response {
    match state
        .auth_service
        .verify_magic_link(&token, None, None, &state.default_tenant_id)
        .await
    {
        Ok((session_token, _user, _tenant_id)) => {
            // Set session cookie
            let cookie =
                create_session_cookie(&session_token, state.auth_config.session_max_age_days);
            cookies.add(cookie);

            // Redirect to app
            Redirect::to("/").into_response()
        }
        Err(e) => {
            let response = VerifyLinkResponse {
                success: false,
                message: format!("Verification failed: {}", e),
                redirect_url: None,
            };
            (StatusCode::BAD_REQUEST, Json(response)).into_response()
        }
    }
}

/// GET /auth/invitations/{token}/accept - Accept an organization invitation.
async fn accept_invitation_handler(
    State(state): State<AuthState>,
    cookies: Cookies,
    Path(token): Path<String>,
) -> Response {
    match state
        .auth_service
        .accept_invitation(&token, None, None)
        .await
    {
        Ok((session_token, _user, _tenant_id)) => {
            // Set session cookie
            let cookie =
                create_session_cookie(&session_token, state.auth_config.session_max_age_days);
            cookies.add(cookie);

            // Redirect to app
            Redirect::to("/").into_response()
        }
        Err(e) => {
            let response = MessageResponse {
                success: false,
                message: format!("Failed to accept invitation: {}", e),
            };
            (StatusCode::BAD_REQUEST, Json(response)).into_response()
        }
    }
}

// ============================================================================
// Protected Route Handlers (require authentication)
// ============================================================================

/// GET /auth/me - Get current authenticated user info.
async fn me_handler(
    State(state): State<AuthState>,
    cookies: Cookies,
) -> Result<Json<MeResponse>, (StatusCode, Json<MessageResponse>)> {
    let auth_user = get_auth_user(&state, &cookies).await?;

    // Get user's organizations
    let orgs = state
        .auth_service
        .get_user_organizations(auth_user.user_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(MessageResponse {
                    success: false,
                    message: format!("Failed to get organizations: {}", e),
                }),
            )
        })?;

    let org_infos: Vec<OrgInfo> = orgs
        .into_iter()
        .map(|m| OrgInfo {
            tenant_id: m.tenant_id,
            name: String::new(), // TODO: fetch tenant names
            role: m.role.as_str().to_string(),
        })
        .collect();

    Ok(Json(MeResponse {
        user_id: auth_user.user_id.to_string(),
        email: auth_user.email,
        email_verified: true, // If they have a session, email is verified
        organizations: org_infos,
    }))
}

/// POST /auth/signout - Sign out and clear session.
async fn signout_handler(
    State(state): State<AuthState>,
    cookies: Cookies,
) -> Result<Json<MessageResponse>, (StatusCode, Json<MessageResponse>)> {
    if let Some(session_token) = extract_session_token(&cookies) {
        let _ = state.auth_service.sign_out(&session_token).await;
    }

    // Clear the cookie regardless
    cookies.remove(clear_session_cookie());

    Ok(Json(MessageResponse {
        success: true,
        message: "Signed out successfully".to_string(),
    }))
}

/// GET /auth/organizations - List user's organizations.
async fn list_organizations_handler(
    State(state): State<AuthState>,
    cookies: Cookies,
) -> Result<Json<ListOrgsResponse>, (StatusCode, Json<MessageResponse>)> {
    let auth_user = get_auth_user(&state, &cookies).await?;

    let orgs = state
        .auth_service
        .get_user_organizations(auth_user.user_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(MessageResponse {
                    success: false,
                    message: format!("Failed to get organizations: {}", e),
                }),
            )
        })?;

    let org_infos: Vec<OrgInfo> = orgs
        .into_iter()
        .map(|m| OrgInfo {
            tenant_id: m.tenant_id,
            name: String::new(), // TODO: fetch tenant names
            role: m.role.as_str().to_string(),
        })
        .collect();

    let count = org_infos.len();
    Ok(Json(ListOrgsResponse {
        organizations: org_infos,
        count,
    }))
}

/// GET /auth/organizations/{tenant_id}/members - List organization members.
async fn list_members_handler(
    State(state): State<AuthState>,
    cookies: Cookies,
    Path(tenant_id): Path<String>,
) -> Result<Json<OrgMembersResponse>, (StatusCode, Json<MessageResponse>)> {
    let auth_user = get_auth_user(&state, &cookies).await?;

    let members = state
        .auth_service
        .get_org_members(&tenant_id, &auth_user)
        .await
        .map_err(|e| {
            let status = if e.to_string().contains("permission") {
                StatusCode::FORBIDDEN
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (
                status,
                Json(MessageResponse {
                    success: false,
                    message: e.to_string(),
                }),
            )
        })?;

    let member_infos: Vec<MemberInfo> = members
        .into_iter()
        .map(|m| MemberInfo {
            user_id: m.user_id.to_string(),
            email: String::new(), // TODO: fetch user emails
            role: m.role.as_str().to_string(),
            joined_at: chrono::DateTime::from_timestamp_millis(m.joined_at)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default(),
        })
        .collect();

    let count = member_infos.len();
    Ok(Json(OrgMembersResponse {
        members: member_infos,
        count,
    }))
}

/// POST /auth/organizations/{tenant_id}/invite - Invite a user to an organization.
async fn invite_user_handler(
    State(state): State<AuthState>,
    cookies: Cookies,
    Path(tenant_id): Path<String>,
    Json(payload): Json<InviteUserRequest>,
) -> Result<Json<InviteUserResponse>, (StatusCode, Json<InviteUserResponse>)> {
    let auth_user = get_auth_user(&state, &cookies)
        .await
        .map_err(|(_status, msg)| {
            (
                StatusCode::UNAUTHORIZED,
                Json(InviteUserResponse {
                    success: false,
                    message: msg.0.message,
                }),
            )
        })?;

    let role = UserRole::parse(&payload.role).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(InviteUserResponse {
                success: false,
                message: format!("Invalid role: {}", payload.role),
            }),
        )
    })?;

    let invite = InviteRequest {
        tenant_id: &tenant_id,
        tenant_name: &tenant_id, // TODO: fetch actual tenant name
        email: &payload.email,
        role,
        inviter: &auth_user,
    };

    state.auth_service.invite_user(&invite).await.map_err(|e| {
        let status = if e.to_string().contains("permission") {
            StatusCode::FORBIDDEN
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        (
            status,
            Json(InviteUserResponse {
                success: false,
                message: e.to_string(),
            }),
        )
    })?;

    Ok(Json(InviteUserResponse {
        success: true,
        message: format!("Invitation sent to {}", payload.email),
    }))
}

/// DELETE /auth/organizations/{tenant_id}/members/{user_id} - Remove a member.
async fn remove_member_handler(
    State(state): State<AuthState>,
    cookies: Cookies,
    Path((tenant_id, user_id)): Path<(String, String)>,
) -> Result<Json<MessageResponse>, (StatusCode, Json<MessageResponse>)> {
    let auth_user = get_auth_user(&state, &cookies).await?;

    let target_user_id = uuid::Uuid::parse_str(&user_id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(MessageResponse {
                success: false,
                message: "Invalid user ID".to_string(),
            }),
        )
    })?;

    state
        .auth_service
        .remove_member(&tenant_id, target_user_id, &auth_user)
        .await
        .map_err(|e| {
            let status = if e.to_string().contains("permission") {
                StatusCode::FORBIDDEN
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (
                status,
                Json(MessageResponse {
                    success: false,
                    message: e.to_string(),
                }),
            )
        })?;

    Ok(Json(MessageResponse {
        success: true,
        message: "Member removed successfully".to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_router_creation() {
        // Just verify the router can be created without panicking
        let _router: Router<AuthState> = auth_router();
    }
}
