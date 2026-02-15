// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

// ============================================================================
// User Status and Role Enums
// ============================================================================

/// Status of a user account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum UserStatus {
    Active,
    Suspended,
    Deleted,
}

impl UserStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            UserStatus::Active => "active",
            UserStatus::Suspended => "suspended",
            UserStatus::Deleted => "deleted",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "active" => Some(UserStatus::Active),
            "suspended" => Some(UserStatus::Suspended),
            "deleted" => Some(UserStatus::Deleted),
            _ => None,
        }
    }
}

/// Role of a user within an organization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum UserRole {
    /// Can delete org, transfer ownership, manage all settings
    Owner,
    /// Can manage members, settings, invite users
    Admin,
    /// Can use search features, view data
    Member,
}

impl UserRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            UserRole::Owner => "owner",
            UserRole::Admin => "admin",
            UserRole::Member => "member",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "owner" => Some(UserRole::Owner),
            "admin" => Some(UserRole::Admin),
            "member" => Some(UserRole::Member),
            _ => None,
        }
    }

    /// Check if this role can invite new members
    pub fn can_invite(&self) -> bool {
        matches!(self, UserRole::Owner | UserRole::Admin)
    }

    /// Check if this role can manage organization settings
    pub fn can_manage_settings(&self) -> bool {
        matches!(self, UserRole::Owner | UserRole::Admin)
    }

    /// Check if this role can remove members
    pub fn can_remove_members(&self) -> bool {
        matches!(self, UserRole::Owner | UserRole::Admin)
    }
}

// ============================================================================
// Database Models
// ============================================================================

/// User record from the database.
#[derive(Debug, Clone)]
pub struct User {
    pub user_id: Uuid,
    pub email: String,
    pub email_verified: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_login_at: Option<i64>,
    pub status: UserStatus,
}

/// Session record from the database.
#[derive(Debug, Clone)]
pub struct Session {
    pub session_id: String, // SHA-256 hash
    pub user_id: Uuid,
    pub tenant_id: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub last_active_at: i64,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
}

impl Session {
    /// Check if the session has expired
    pub fn is_expired(&self) -> bool {
        let now = chrono::Utc::now().timestamp_millis();
        self.expires_at < now
    }
}

/// Magic link token record from the database.
#[derive(Debug, Clone)]
pub struct MagicLinkToken {
    pub token: String, // SHA-256 hash
    pub email: String,
    pub tenant_id: Option<String>,
    pub redirect_url: Option<String>,
    pub created_at: i64,
    pub expires_at: i64,
    pub used: bool,
}

impl MagicLinkToken {
    /// Check if the token has expired
    pub fn is_expired(&self) -> bool {
        let now = chrono::Utc::now().timestamp_millis();
        self.expires_at < now
    }

    /// Check if the token is valid (not expired and not used)
    pub fn is_valid(&self) -> bool {
        !self.used && !self.is_expired()
    }
}

/// Organization membership record.
#[derive(Debug, Clone)]
pub struct OrgMembership {
    pub tenant_id: String,
    pub user_id: Uuid,
    pub role: UserRole,
    pub joined_at: i64,
    pub invited_by: Option<Uuid>,
}

/// Organization invitation record.
#[derive(Debug, Clone)]
pub struct OrgInvitation {
    pub token: String, // SHA-256 hash
    pub tenant_id: String,
    pub email: String,
    pub role: UserRole,
    pub invited_by: Uuid,
    pub created_at: i64,
    pub expires_at: i64,
    pub accepted: bool,
}

impl OrgInvitation {
    /// Check if the invitation has expired
    pub fn is_expired(&self) -> bool {
        let now = chrono::Utc::now().timestamp_millis();
        self.expires_at < now
    }

    /// Check if the invitation is valid (not expired and not accepted)
    pub fn is_valid(&self) -> bool {
        !self.accepted && !self.is_expired()
    }
}

// ============================================================================
// API Request Types
// ============================================================================

/// Request to send a magic link for authentication.
#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct RequestLinkRequest {
    /// Email address to send the magic link to
    pub email: String,
    /// Optional: organization name when creating a new org (multi-tenant only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub org_name: Option<String>,
}

/// Request to invite a user to an organization.
#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct InviteUserRequest {
    /// Email address of the user to invite
    pub email: String,
    /// Role to assign to the user (owner, admin, member)
    pub role: String,
}

/// Request to create a new organization.
#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct CreateOrgRequest {
    /// Name of the organization
    pub name: String,
}

// ============================================================================
// API Response Types
// ============================================================================

/// Response after requesting a magic link.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct RequestLinkResponse {
    pub success: bool,
    pub message: String,
}

/// Response after verifying a magic link - includes session info.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VerifyLinkResponse {
    pub success: bool,
    pub message: String,
    /// Redirect URL after successful verification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_url: Option<String>,
}

/// Current authenticated user information.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct MeResponse {
    pub user_id: String,
    pub email: String,
    pub email_verified: bool,
    /// List of organizations the user belongs to
    pub organizations: Vec<OrgInfo>,
}

/// Information about an organization.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct OrgInfo {
    pub tenant_id: String,
    pub name: String,
    pub role: String,
}

/// Response for listing organization members.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct OrgMembersResponse {
    pub members: Vec<MemberInfo>,
    pub count: usize,
}

/// Information about an organization member.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct MemberInfo {
    pub user_id: String,
    pub email: String,
    pub role: String,
    pub joined_at: String,
}

/// Response after inviting a user.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct InviteUserResponse {
    pub success: bool,
    pub message: String,
}

/// Response for listing organizations.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ListOrgsResponse {
    pub organizations: Vec<OrgInfo>,
    pub count: usize,
}

/// Response after creating an organization.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CreateOrgResponse {
    pub success: bool,
    pub message: String,
    pub tenant_id: String,
}

/// Generic message response.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct MessageResponse {
    pub success: bool,
    pub message: String,
}

/// Invitation details response.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct InvitationDetailsResponse {
    pub org_name: String,
    pub email: String,
    pub role: String,
    pub invited_by_email: String,
    pub expires_at: String,
}

// ============================================================================
// Authenticated User Context
// ============================================================================

/// Authenticated user context extracted from session cookie.
/// This is attached to requests by the auth middleware.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: Uuid,
    pub email: String,
    pub tenant_id: String,
    pub role: UserRole,
}

impl AuthUser {
    /// Check if the user can invite members to the current organization
    pub fn can_invite(&self) -> bool {
        self.role.can_invite()
    }

    /// Check if the user can manage settings for the current organization
    pub fn can_manage_settings(&self) -> bool {
        self.role.can_manage_settings()
    }

    /// Check if the user can remove members from the current organization
    pub fn can_remove_members(&self) -> bool {
        self.role.can_remove_members()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_status_roundtrip() {
        assert_eq!(
            UserStatus::parse(UserStatus::Active.as_str()),
            Some(UserStatus::Active)
        );
        assert_eq!(
            UserStatus::parse(UserStatus::Suspended.as_str()),
            Some(UserStatus::Suspended)
        );
        assert_eq!(
            UserStatus::parse(UserStatus::Deleted.as_str()),
            Some(UserStatus::Deleted)
        );
    }

    #[test]
    fn test_user_status_invalid() {
        assert_eq!(UserStatus::parse("invalid"), None);
    }

    #[test]
    fn test_user_role_roundtrip() {
        assert_eq!(
            UserRole::parse(UserRole::Owner.as_str()),
            Some(UserRole::Owner)
        );
        assert_eq!(
            UserRole::parse(UserRole::Admin.as_str()),
            Some(UserRole::Admin)
        );
        assert_eq!(
            UserRole::parse(UserRole::Member.as_str()),
            Some(UserRole::Member)
        );
    }

    #[test]
    fn test_user_role_invalid() {
        assert_eq!(UserRole::parse("superadmin"), None);
    }

    #[test]
    fn test_role_permissions() {
        assert!(UserRole::Owner.can_invite());
        assert!(UserRole::Owner.can_manage_settings());
        assert!(UserRole::Owner.can_remove_members());

        assert!(UserRole::Admin.can_invite());
        assert!(UserRole::Admin.can_manage_settings());
        assert!(UserRole::Admin.can_remove_members());

        assert!(!UserRole::Member.can_invite());
        assert!(!UserRole::Member.can_manage_settings());
        assert!(!UserRole::Member.can_remove_members());
    }

    #[test]
    fn test_session_expiry() {
        let expired_session = Session {
            session_id: "test".to_string(),
            user_id: Uuid::new_v4(),
            tenant_id: "default".to_string(),
            created_at: 0,
            expires_at: 0, // Expired at epoch
            last_active_at: 0,
            user_agent: None,
            ip_address: None,
        };
        assert!(expired_session.is_expired());

        let future_timestamp = chrono::Utc::now().timestamp_millis() + 3600000; // 1 hour from now
        let valid_session = Session {
            session_id: "test".to_string(),
            user_id: Uuid::new_v4(),
            tenant_id: "default".to_string(),
            created_at: 0,
            expires_at: future_timestamp,
            last_active_at: 0,
            user_agent: None,
            ip_address: None,
        };
        assert!(!valid_session.is_expired());
    }

    #[test]
    fn test_magic_link_validity() {
        let future_timestamp = chrono::Utc::now().timestamp_millis() + 3600000;

        let valid_token = MagicLinkToken {
            token: "test".to_string(),
            email: "test@example.com".to_string(),
            tenant_id: None,
            redirect_url: None,
            created_at: 0,
            expires_at: future_timestamp,
            used: false,
        };
        assert!(valid_token.is_valid());

        let used_token = MagicLinkToken {
            token: "test".to_string(),
            email: "test@example.com".to_string(),
            tenant_id: None,
            redirect_url: None,
            created_at: 0,
            expires_at: future_timestamp,
            used: true,
        };
        assert!(!used_token.is_valid());

        let expired_token = MagicLinkToken {
            token: "test".to_string(),
            email: "test@example.com".to_string(),
            tenant_id: None,
            redirect_url: None,
            created_at: 0,
            expires_at: 0,
            used: false,
        };
        assert!(!expired_token.is_valid());
    }
}
