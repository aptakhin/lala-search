// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

//! Database operations for authentication.
//!
//! Auth data (users, sessions, tokens, invitations, tenants) is global — not
//! scoped to a single tenant — so these queries do NOT use RLS.

use crate::models::auth::{
    MagicLinkSendDecision, MagicLinkSendThrottle, MagicLinkToken, OrgInvitation, OrgMembership,
    Session, User, UserRole, UserStatus,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::postgres::PgPool;
use sqlx::Row;
use uuid::Uuid;

/// Parameters for creating a session.
pub struct CreateSessionParams<'a> {
    pub session_id_hash: &'a str,
    pub user_id: Uuid,
    pub tenant_id: Uuid,
    pub expires_at: DateTime<Utc>,
    pub user_agent: Option<&'a str>,
    pub ip_address: Option<&'a str>,
}

/// Parameters for creating a magic link token.
pub struct CreateMagicLinkParams<'a> {
    pub token_hash: &'a str,
    pub email: &'a str,
    pub tenant_id: Option<Uuid>,
    pub redirect_url: Option<&'a str>,
    pub expires_at: DateTime<Utc>,
}

/// Parameters for creating an invitation.
pub struct CreateInvitationParams<'a> {
    pub token_hash: &'a str,
    pub tenant_id: Uuid,
    pub email: &'a str,
    pub role: UserRole,
    pub invited_by: Uuid,
    pub expires_at: DateTime<Utc>,
}

/// Authentication database client.
///
/// Wraps a PostgreSQL connection pool and provides auth-specific operations.
/// Auth tables are global (no RLS) — tenant_id is an explicit column.
#[derive(Clone)]
pub struct AuthDbClient {
    pool: PgPool,
}

impl AuthDbClient {
    /// Create a new auth database client.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Get a reference to the underlying connection pool.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    // ========== User Operations ==========

    /// Get a user by email (active users only).
    pub async fn get_user_by_email(&self, email: &str) -> Result<Option<User>> {
        let row = sqlx::query(
            "SELECT user_id, email, email_verified, created_at, updated_at, last_login_at, status
             FROM users WHERE email = $1 AND deleted_at IS NULL",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| "Failed to get user by email".to_string())?;

        Ok(row.map(|r| {
            let status_str: String = r.get("status");
            User {
                user_id: r.get("user_id"),
                email: r.get("email"),
                email_verified: r.get("email_verified"),
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
                last_login_at: r.get("last_login_at"),
                status: UserStatus::parse(&status_str).unwrap_or(UserStatus::Active),
            }
        }))
    }

    /// Get a user by ID (active users only).
    pub async fn get_user_by_id(&self, user_id: Uuid) -> Result<Option<User>> {
        let row = sqlx::query(
            "SELECT user_id, email, email_verified, created_at, updated_at, last_login_at, status
             FROM users WHERE user_id = $1 AND deleted_at IS NULL",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("Failed to get user by id: {user_id}"))?;

        Ok(row.map(|r| {
            let status_str: String = r.get("status");
            User {
                user_id: r.get("user_id"),
                email: r.get("email"),
                email_verified: r.get("email_verified"),
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
                last_login_at: r.get("last_login_at"),
                status: UserStatus::parse(&status_str).unwrap_or(UserStatus::Active),
            }
        }))
    }

    /// Get multiple users by their IDs in a single query (active users only).
    pub async fn get_users_by_ids(&self, user_ids: Vec<Uuid>) -> Result<Vec<User>> {
        if user_ids.is_empty() {
            return Ok(Vec::new());
        }

        let rows = sqlx::query(
            "SELECT user_id, email, email_verified, created_at, updated_at, last_login_at, status
             FROM users WHERE user_id = ANY($1) AND deleted_at IS NULL",
        )
        .bind(&user_ids)
        .fetch_all(&self.pool)
        .await
        .context("Failed to batch-fetch users by IDs")?;

        Ok(rows
            .iter()
            .map(|r| {
                let status_str: String = r.get("status");
                User {
                    user_id: r.get("user_id"),
                    email: r.get("email"),
                    email_verified: r.get("email_verified"),
                    created_at: r.get("created_at"),
                    updated_at: r.get("updated_at"),
                    last_login_at: r.get("last_login_at"),
                    status: UserStatus::parse(&status_str).unwrap_or(UserStatus::Active),
                }
            })
            .collect())
    }

    /// Create a new user.
    pub async fn create_user(&self, email: &str) -> Result<Uuid> {
        let user_id = Uuid::now_v7();

        sqlx::query(
            "INSERT INTO users (user_id, email, email_verified, status)
             VALUES ($1, $2, FALSE, 'active')",
        )
        .bind(user_id)
        .bind(email)
        .execute(&self.pool)
        .await
        .with_context(|| "Failed to create user".to_string())?;

        Ok(user_id)
    }

    /// Update user's last login time.
    pub async fn update_user_last_login(&self, user_id: Uuid) -> Result<()> {
        sqlx::query(
            "UPDATE users SET last_login_at = now(), updated_at = now() WHERE user_id = $1",
        )
        .bind(user_id)
        .execute(&self.pool)
        .await
        .with_context(|| format!("Failed to update last login for user: {user_id}"))?;

        Ok(())
    }

    /// Mark user's email as verified.
    pub async fn set_user_email_verified(&self, user_id: Uuid) -> Result<()> {
        sqlx::query(
            "UPDATE users SET email_verified = TRUE, updated_at = now() WHERE user_id = $1",
        )
        .bind(user_id)
        .execute(&self.pool)
        .await
        .with_context(|| format!("Failed to set email verified for user: {user_id}"))?;

        Ok(())
    }

    // ========== Session Operations ==========

    /// Create a new session.
    pub async fn create_session(&self, params: &CreateSessionParams<'_>) -> Result<()> {
        sqlx::query(
            "INSERT INTO sessions
             (session_id, user_id, tenant_id, expires_at, user_agent, ip_address)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(params.session_id_hash)
        .bind(params.user_id)
        .bind(params.tenant_id)
        .bind(params.expires_at)
        .bind(params.user_agent)
        .bind(params.ip_address)
        .execute(&self.pool)
        .await
        .context("Failed to create session")?;

        Ok(())
    }

    /// Get a session by its hash.
    pub async fn get_session(&self, session_id_hash: &str) -> Result<Option<Session>> {
        let row = sqlx::query(
            "SELECT session_id, user_id, tenant_id, created_at, expires_at, last_active_at,
                    user_agent, ip_address
             FROM sessions WHERE session_id = $1",
        )
        .bind(session_id_hash)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get session")?;

        Ok(row.map(|r| Session {
            session_id: r.get("session_id"),
            user_id: r.get("user_id"),
            tenant_id: r.get("tenant_id"),
            created_at: r.get("created_at"),
            expires_at: r.get("expires_at"),
            last_active_at: r.get("last_active_at"),
            user_agent: r.get("user_agent"),
            ip_address: r.get("ip_address"),
        }))
    }

    /// Delete a session.
    pub async fn delete_session(&self, session_id_hash: &str) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE session_id = $1")
            .bind(session_id_hash)
            .execute(&self.pool)
            .await
            .context("Failed to delete session")?;

        Ok(())
    }

    /// Delete all sessions for a user (single query — no ALLOW FILTERING needed).
    pub async fn delete_user_sessions(&self, user_id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE user_id = $1")
            .bind(user_id)
            .execute(&self.pool)
            .await
            .with_context(|| format!("Failed to delete sessions for user: {user_id}"))?;

        Ok(())
    }

    /// Update session's last active time.
    pub async fn touch_session(&self, session_id_hash: &str) -> Result<()> {
        sqlx::query("UPDATE sessions SET last_active_at = now() WHERE session_id = $1")
            .bind(session_id_hash)
            .execute(&self.pool)
            .await
            .context("Failed to touch session")?;

        Ok(())
    }

    // ========== Magic Link Token Operations ==========

    /// Create a magic link token.
    pub async fn create_magic_link_token(&self, params: &CreateMagicLinkParams<'_>) -> Result<()> {
        sqlx::query(
            "INSERT INTO magic_link_tokens
             (token_hash, email, tenant_id, redirect_url, expires_at)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(params.token_hash)
        .bind(params.email)
        .bind(params.tenant_id)
        .bind(params.redirect_url)
        .bind(params.expires_at)
        .execute(&self.pool)
        .await
        .context("Failed to create magic link token")?;

        Ok(())
    }

    /// Reserve a magic link send slot for an email address.
    pub async fn consume_magic_link_send_attempt(
        &self,
        email: &str,
        now: DateTime<Utc>,
        cooldown: chrono::Duration,
        max_attempts: i32,
        window: chrono::Duration,
    ) -> Result<MagicLinkSendDecision> {
        let mut tx = self.pool.begin().await.with_context(|| {
            format!("Failed to start magic link throttle transaction for {email}")
        })?;

        let existing = sqlx::query(
            "SELECT email, first_attempt_at, last_attempt_at, blocked_until, attempt_count
             FROM magic_link_send_attempts
             WHERE email = $1
             FOR UPDATE",
        )
        .bind(email)
        .fetch_optional(&mut *tx)
        .await
        .with_context(|| format!("Failed to load magic link throttle state for {email}"))?;

        let existing = existing.map(|r| MagicLinkSendThrottle {
            email: r.get("email"),
            first_attempt_at: r.get("first_attempt_at"),
            last_attempt_at: r.get("last_attempt_at"),
            blocked_until: r.get("blocked_until"),
            attempt_count: r.get("attempt_count"),
        });

        let decision = existing
            .as_ref()
            .map(|throttle| throttle.evaluate_send(now, cooldown, max_attempts, window))
            .unwrap_or(MagicLinkSendDecision::Allow);

        if decision != MagicLinkSendDecision::Allow {
            tx.commit().await.with_context(|| {
                format!("Failed to finish magic link throttle transaction for {email}")
            })?;
            return Ok(decision);
        }

        let (first_attempt_at, attempt_count, blocked_until) = match existing {
            Some(throttle) if now < throttle.first_attempt_at + window => {
                let next_attempt_count = throttle.attempt_count + 1;
                let window_expires_at = throttle.first_attempt_at + window;
                (
                    throttle.first_attempt_at,
                    next_attempt_count,
                    (next_attempt_count >= max_attempts).then_some(window_expires_at),
                )
            }
            _ => (now, 1, (max_attempts <= 1).then_some(now + window)),
        };

        sqlx::query(
            "INSERT INTO magic_link_send_attempts
             (email, first_attempt_at, last_attempt_at, blocked_until, attempt_count)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (email) DO UPDATE SET
                first_attempt_at = EXCLUDED.first_attempt_at,
                last_attempt_at = EXCLUDED.last_attempt_at,
                blocked_until = EXCLUDED.blocked_until,
                attempt_count = EXCLUDED.attempt_count",
        )
        .bind(email)
        .bind(first_attempt_at)
        .bind(now)
        .bind(blocked_until)
        .bind(attempt_count)
        .execute(&mut *tx)
        .await
        .with_context(|| format!("Failed to store magic link throttle state for {email}"))?;

        tx.commit()
            .await
            .with_context(|| format!("Failed to commit magic link throttle state for {email}"))?;

        Ok(MagicLinkSendDecision::Allow)
    }

    /// Get a magic link token.
    pub async fn get_magic_link_token(&self, token_hash: &str) -> Result<Option<MagicLinkToken>> {
        let row = sqlx::query(
            "SELECT token_hash, email, tenant_id, redirect_url, created_at, expires_at, used
             FROM magic_link_tokens WHERE token_hash = $1",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get magic link token")?;

        Ok(row.map(|r| MagicLinkToken {
            token: r.get("token_hash"),
            email: r.get("email"),
            tenant_id: r.get("tenant_id"),
            redirect_url: r.get("redirect_url"),
            created_at: r.get("created_at"),
            expires_at: r.get("expires_at"),
            used: r.get("used"),
        }))
    }

    /// Mark a magic link token as used.
    pub async fn mark_magic_link_used(&self, token_hash: &str) -> Result<()> {
        sqlx::query("UPDATE magic_link_tokens SET used = TRUE WHERE token_hash = $1")
            .bind(token_hash)
            .execute(&self.pool)
            .await
            .context("Failed to mark magic link as used")?;

        Ok(())
    }

    // ========== Organization Membership Operations ==========

    /// Add a user to an organization.
    /// Uses ON CONFLICT to re-activate soft-deleted memberships.
    pub async fn add_org_membership(
        &self,
        tenant_id: Uuid,
        user_id: Uuid,
        role: UserRole,
        invited_by: Option<Uuid>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO org_memberships (tenant_id, user_id, role, invited_by)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (tenant_id, user_id) DO UPDATE SET
                role = $3, invited_by = $4, deleted_at = NULL, joined_at = now()",
        )
        .bind(tenant_id)
        .bind(user_id)
        .bind(role.as_str())
        .bind(invited_by)
        .execute(&self.pool)
        .await
        .context("Failed to add org membership")?;

        Ok(())
    }

    /// Get a user's membership in an organization (active only).
    pub async fn get_org_membership(
        &self,
        tenant_id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<OrgMembership>> {
        let row = sqlx::query(
            "SELECT tenant_id, user_id, role, joined_at, invited_by
             FROM org_memberships
             WHERE tenant_id = $1 AND user_id = $2 AND deleted_at IS NULL",
        )
        .bind(tenant_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get org membership")?;

        Ok(row.map(|r| {
            let role_str: String = r.get("role");
            OrgMembership {
                tenant_id: r.get("tenant_id"),
                user_id: r.get("user_id"),
                role: UserRole::parse(&role_str).unwrap_or(UserRole::Member),
                joined_at: r.get("joined_at"),
                invited_by: r.get("invited_by"),
            }
        }))
    }

    /// Get all organizations a user belongs to (active memberships only).
    /// Uses idx_org_memberships_user index for efficient lookup by user_id.
    pub async fn get_user_orgs(&self, user_id: Uuid) -> Result<Vec<OrgMembership>> {
        let rows = sqlx::query(
            "SELECT tenant_id, user_id, role, joined_at, invited_by
             FROM org_memberships
             WHERE user_id = $1 AND deleted_at IS NULL",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .with_context(|| format!("Failed to get orgs for user: {user_id}"))?;

        Ok(rows
            .iter()
            .map(|r| {
                let role_str: String = r.get("role");
                OrgMembership {
                    tenant_id: r.get("tenant_id"),
                    user_id: r.get("user_id"),
                    role: UserRole::parse(&role_str).unwrap_or(UserRole::Member),
                    joined_at: r.get("joined_at"),
                    invited_by: r.get("invited_by"),
                }
            })
            .collect())
    }

    /// Get all members of an organization (active memberships only).
    pub async fn get_org_members(&self, tenant_id: Uuid) -> Result<Vec<OrgMembership>> {
        let rows = sqlx::query(
            "SELECT tenant_id, user_id, role, joined_at, invited_by
             FROM org_memberships
             WHERE tenant_id = $1 AND deleted_at IS NULL",
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await
        .with_context(|| format!("Failed to get members for tenant: {tenant_id}"))?;

        Ok(rows
            .iter()
            .map(|r| {
                let role_str: String = r.get("role");
                OrgMembership {
                    tenant_id: r.get("tenant_id"),
                    user_id: r.get("user_id"),
                    role: UserRole::parse(&role_str).unwrap_or(UserRole::Member),
                    joined_at: r.get("joined_at"),
                    invited_by: r.get("invited_by"),
                }
            })
            .collect())
    }

    /// Soft-delete a user's membership in an organization.
    pub async fn remove_org_membership(&self, tenant_id: Uuid, user_id: Uuid) -> Result<()> {
        sqlx::query(
            "UPDATE org_memberships SET deleted_at = now()
             WHERE tenant_id = $1 AND user_id = $2 AND deleted_at IS NULL",
        )
        .bind(tenant_id)
        .bind(user_id)
        .execute(&self.pool)
        .await
        .context("Failed to remove org membership")?;

        Ok(())
    }

    /// Hard-delete a membership (for test cleanup only).
    pub async fn hard_delete_org_membership(&self, tenant_id: Uuid, user_id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM org_memberships WHERE tenant_id = $1 AND user_id = $2")
            .bind(tenant_id)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .context("Failed to hard-delete org membership")?;

        Ok(())
    }

    // ========== Organization Invitation Operations ==========

    /// Create an organization invitation.
    pub async fn create_invitation(&self, params: &CreateInvitationParams<'_>) -> Result<()> {
        sqlx::query(
            "INSERT INTO org_invitations
             (token_hash, tenant_id, email, role, invited_by, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(params.token_hash)
        .bind(params.tenant_id)
        .bind(params.email)
        .bind(params.role.as_str())
        .bind(params.invited_by)
        .bind(params.expires_at)
        .execute(&self.pool)
        .await
        .context("Failed to create invitation")?;

        Ok(())
    }

    /// Get an invitation by token.
    pub async fn get_invitation(&self, token_hash: &str) -> Result<Option<OrgInvitation>> {
        let row = sqlx::query(
            "SELECT token_hash, tenant_id, email, role, invited_by, created_at, expires_at, accepted
             FROM org_invitations WHERE token_hash = $1",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get invitation")?;

        Ok(row.map(|r| {
            let role_str: String = r.get("role");
            OrgInvitation {
                token: r.get("token_hash"),
                tenant_id: r.get("tenant_id"),
                email: r.get("email"),
                role: UserRole::parse(&role_str).unwrap_or(UserRole::Member),
                invited_by: r.get("invited_by"),
                created_at: r.get("created_at"),
                expires_at: r.get("expires_at"),
                accepted: r.get("accepted"),
            }
        }))
    }

    /// Mark an invitation as accepted.
    pub async fn mark_invitation_accepted(&self, token_hash: &str) -> Result<()> {
        sqlx::query("UPDATE org_invitations SET accepted = TRUE WHERE token_hash = $1")
            .bind(token_hash)
            .execute(&self.pool)
            .await
            .context("Failed to mark invitation accepted")?;

        Ok(())
    }

    // ========== Tenant Operations ==========

    /// Create a new tenant.
    pub async fn create_tenant(&self, tenant_id: Uuid, name: &str) -> Result<()> {
        sqlx::query("INSERT INTO tenants (tenant_id, name) VALUES ($1, $2)")
            .bind(tenant_id)
            .bind(name)
            .execute(&self.pool)
            .await
            .with_context(|| format!("Failed to create tenant: {tenant_id}"))?;

        Ok(())
    }

    /// Get tenant name by ID (active tenants only).
    pub async fn get_tenant_name(&self, tenant_id: Uuid) -> Result<Option<String>> {
        let row =
            sqlx::query("SELECT name FROM tenants WHERE tenant_id = $1 AND deleted_at IS NULL")
                .bind(tenant_id)
                .fetch_optional(&self.pool)
                .await
                .with_context(|| format!("Failed to get tenant name: {tenant_id}"))?;

        Ok(row.map(|r| r.get("name")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::auth::{UserRole, UserStatus};

    // ========== Unit Tests (no external dependencies) ==========

    #[test]
    fn test_user_status_parse_valid() {
        assert_eq!(UserStatus::parse("active"), Some(UserStatus::Active));
        assert_eq!(UserStatus::parse("suspended"), Some(UserStatus::Suspended));
        assert_eq!(UserStatus::parse("deleted"), Some(UserStatus::Deleted));
    }

    #[test]
    fn test_user_status_parse_invalid() {
        assert_eq!(UserStatus::parse("unknown"), None);
        assert_eq!(UserStatus::parse(""), None);
    }

    #[test]
    fn test_user_status_as_str_roundtrip() {
        for status in [
            UserStatus::Active,
            UserStatus::Suspended,
            UserStatus::Deleted,
        ] {
            let s = status.as_str();
            assert_eq!(UserStatus::parse(s), Some(status));
        }
    }

    #[test]
    fn test_user_role_parse_valid() {
        assert_eq!(UserRole::parse("owner"), Some(UserRole::Owner));
        assert_eq!(UserRole::parse("admin"), Some(UserRole::Admin));
        assert_eq!(UserRole::parse("member"), Some(UserRole::Member));
    }

    #[test]
    fn test_user_role_parse_invalid() {
        assert_eq!(UserRole::parse("superuser"), None);
        assert_eq!(UserRole::parse(""), None);
    }

    #[test]
    fn test_user_role_as_str_roundtrip() {
        for role in [UserRole::Owner, UserRole::Admin, UserRole::Member] {
            let s = role.as_str();
            assert_eq!(UserRole::parse(s), Some(role));
        }
    }

    #[test]
    fn test_role_permissions_owner() {
        let role = UserRole::Owner;
        assert!(role.can_invite());
        assert!(role.can_manage_settings());
        assert!(role.can_remove_members());
    }

    #[test]
    fn test_role_permissions_admin() {
        let role = UserRole::Admin;
        assert!(role.can_invite());
        assert!(role.can_manage_settings());
        assert!(role.can_remove_members());
    }

    #[test]
    fn test_role_permissions_member() {
        let role = UserRole::Member;
        assert!(!role.can_invite());
        assert!(!role.can_manage_settings());
        assert!(!role.can_remove_members());
    }

    // ========== Integration Tests (require PostgreSQL) ==========

    /// Helper to create an AuthDbClient for tests.
    async fn create_test_client() -> AuthDbClient {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for tests");
        let pool = PgPool::connect(&database_url)
            .await
            .expect("Failed to connect to PostgreSQL");

        AuthDbClient::new(pool)
    }

    /// Helper to create a test tenant and return its UUID.
    async fn create_test_tenant(client: &AuthDbClient) -> Uuid {
        let tenant_id = Uuid::now_v7();
        client
            .create_tenant(tenant_id, "Test Tenant")
            .await
            .expect("Failed to create test tenant");
        tenant_id
    }

    #[tokio::test]
    #[ignore]
    async fn test_create_and_get_user_by_email() {
        let client = create_test_client().await;
        let test_email = format!("test-{}@example.com", Uuid::now_v7());

        let user_id = client
            .create_user(&test_email)
            .await
            .expect("Failed to create user");

        let user = client
            .get_user_by_email(&test_email)
            .await
            .expect("Failed to get user")
            .expect("User should exist");

        assert_eq!(user.user_id, user_id);
        assert_eq!(user.email, test_email);
        assert!(!user.email_verified);
        assert_eq!(user.status, UserStatus::Active);

        // Cleanup
        sqlx::query("DELETE FROM users WHERE user_id = $1")
            .bind(user_id)
            .execute(client.pool())
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_create_and_get_user_by_id() {
        let client = create_test_client().await;
        let test_email = format!("test-{}@example.com", Uuid::now_v7());

        let user_id = client
            .create_user(&test_email)
            .await
            .expect("Failed to create user");

        let user = client
            .get_user_by_id(user_id)
            .await
            .expect("Failed to get user")
            .expect("User should exist");

        assert_eq!(user.user_id, user_id);
        assert_eq!(user.email, test_email);

        // Cleanup
        sqlx::query("DELETE FROM users WHERE user_id = $1")
            .bind(user_id)
            .execute(client.pool())
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_nonexistent_user_returns_none() {
        let client = create_test_client().await;

        let result = client
            .get_user_by_email("nonexistent@example.com")
            .await
            .expect("Query should succeed");

        assert!(result.is_none());
    }

    #[tokio::test]
    #[ignore]
    async fn test_update_user_last_login() {
        let client = create_test_client().await;
        let test_email = format!("test-{}@example.com", Uuid::now_v7());

        let user_id = client
            .create_user(&test_email)
            .await
            .expect("Failed to create user");

        let user = client
            .get_user_by_id(user_id)
            .await
            .expect("Failed to get user")
            .expect("User should exist");
        assert!(user.last_login_at.is_none());

        client
            .update_user_last_login(user_id)
            .await
            .expect("Failed to update last login");

        let user = client
            .get_user_by_id(user_id)
            .await
            .expect("Failed to get user")
            .expect("User should exist");
        assert!(user.last_login_at.is_some());

        // Cleanup
        sqlx::query("DELETE FROM users WHERE user_id = $1")
            .bind(user_id)
            .execute(client.pool())
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_set_user_email_verified() {
        let client = create_test_client().await;
        let test_email = format!("test-{}@example.com", Uuid::now_v7());

        let user_id = client
            .create_user(&test_email)
            .await
            .expect("Failed to create user");

        let user = client
            .get_user_by_id(user_id)
            .await
            .expect("Failed to get user")
            .expect("User should exist");
        assert!(!user.email_verified);

        client
            .set_user_email_verified(user_id)
            .await
            .expect("Failed to set email verified");

        let user = client
            .get_user_by_id(user_id)
            .await
            .expect("Failed to get user")
            .expect("User should exist");
        assert!(user.email_verified);

        // Cleanup
        sqlx::query("DELETE FROM users WHERE user_id = $1")
            .bind(user_id)
            .execute(client.pool())
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_create_and_get_session() {
        let client = create_test_client().await;
        let tenant_id = create_test_tenant(&client).await;
        let test_email = format!("test-{}@example.com", Uuid::now_v7());
        let user_id = client
            .create_user(&test_email)
            .await
            .expect("Failed to create user");

        let session_hash = format!("test-session-{}", Uuid::now_v7());
        let expires_at = Utc::now() + chrono::Duration::days(1);

        client
            .create_session(&CreateSessionParams {
                session_id_hash: &session_hash,
                user_id,
                tenant_id,
                expires_at,
                user_agent: None,
                ip_address: None,
            })
            .await
            .expect("Failed to create session");

        let session = client
            .get_session(&session_hash)
            .await
            .expect("Failed to get session")
            .expect("Session should exist");

        assert_eq!(session.session_id, session_hash);
        assert_eq!(session.user_id, user_id);
        assert_eq!(session.tenant_id, tenant_id);
        assert!(!session.is_expired());

        // Cleanup
        client.delete_session(&session_hash).await.ok();
        sqlx::query("DELETE FROM users WHERE user_id = $1")
            .bind(user_id)
            .execute(client.pool())
            .await
            .ok();
        sqlx::query("DELETE FROM tenants WHERE tenant_id = $1")
            .bind(tenant_id)
            .execute(client.pool())
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_session_expiry_check() {
        let client = create_test_client().await;
        let tenant_id = create_test_tenant(&client).await;
        let test_email = format!("test-{}@example.com", Uuid::now_v7());
        let user_id = client
            .create_user(&test_email)
            .await
            .expect("Failed to create user");

        let session_hash = format!("test-session-{}", Uuid::now_v7());
        let expires_at = Utc::now() - chrono::Duration::seconds(10); // Already expired

        client
            .create_session(&CreateSessionParams {
                session_id_hash: &session_hash,
                user_id,
                tenant_id,
                expires_at,
                user_agent: None,
                ip_address: None,
            })
            .await
            .expect("Failed to create session");

        let session = client
            .get_session(&session_hash)
            .await
            .expect("Failed to get session")
            .expect("Session should exist");

        assert!(session.is_expired());

        // Cleanup
        client.delete_session(&session_hash).await.ok();
        sqlx::query("DELETE FROM users WHERE user_id = $1")
            .bind(user_id)
            .execute(client.pool())
            .await
            .ok();
        sqlx::query("DELETE FROM tenants WHERE tenant_id = $1")
            .bind(tenant_id)
            .execute(client.pool())
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_delete_session() {
        let client = create_test_client().await;
        let tenant_id = create_test_tenant(&client).await;
        let test_email = format!("test-{}@example.com", Uuid::now_v7());
        let user_id = client
            .create_user(&test_email)
            .await
            .expect("Failed to create user");

        let session_hash = format!("test-session-{}", Uuid::now_v7());
        let expires_at = Utc::now() + chrono::Duration::days(1);

        client
            .create_session(&CreateSessionParams {
                session_id_hash: &session_hash,
                user_id,
                tenant_id,
                expires_at,
                user_agent: None,
                ip_address: None,
            })
            .await
            .expect("Failed to create session");

        client
            .delete_session(&session_hash)
            .await
            .expect("Failed to delete session");

        let result = client
            .get_session(&session_hash)
            .await
            .expect("Query should succeed");
        assert!(result.is_none());

        // Cleanup
        sqlx::query("DELETE FROM users WHERE user_id = $1")
            .bind(user_id)
            .execute(client.pool())
            .await
            .ok();
        sqlx::query("DELETE FROM tenants WHERE tenant_id = $1")
            .bind(tenant_id)
            .execute(client.pool())
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_create_and_get_magic_link_token() {
        let client = create_test_client().await;
        let token_hash = format!("test-token-{}", Uuid::now_v7());
        let email = "test@example.com";
        let expires_at = Utc::now() + chrono::Duration::minutes(15);

        client
            .create_magic_link_token(&CreateMagicLinkParams {
                token_hash: &token_hash,
                email,
                tenant_id: None,
                redirect_url: None,
                expires_at,
            })
            .await
            .expect("Failed to create magic link token");

        let token = client
            .get_magic_link_token(&token_hash)
            .await
            .expect("Failed to get token")
            .expect("Token should exist");

        assert_eq!(token.token, token_hash);
        assert_eq!(token.email, email);
        assert!(!token.used);
        assert!(token.is_valid());

        // Cleanup
        sqlx::query("DELETE FROM magic_link_tokens WHERE token_hash = $1")
            .bind(&token_hash)
            .execute(client.pool())
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_mark_magic_link_used() {
        let client = create_test_client().await;
        let token_hash = format!("test-token-{}", Uuid::now_v7());
        let expires_at = Utc::now() + chrono::Duration::minutes(15);

        client
            .create_magic_link_token(&CreateMagicLinkParams {
                token_hash: &token_hash,
                email: "test@example.com",
                tenant_id: None,
                redirect_url: None,
                expires_at,
            })
            .await
            .expect("Failed to create magic link token");

        client
            .mark_magic_link_used(&token_hash)
            .await
            .expect("Failed to mark token as used");

        let token = client
            .get_magic_link_token(&token_hash)
            .await
            .expect("Failed to get token")
            .expect("Token should exist");
        assert!(token.used);
        assert!(!token.is_valid());

        // Cleanup
        sqlx::query("DELETE FROM magic_link_tokens WHERE token_hash = $1")
            .bind(&token_hash)
            .execute(client.pool())
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_add_and_get_org_membership() {
        let client = create_test_client().await;
        let tenant_id = create_test_tenant(&client).await;
        let test_email = format!("test-{}@example.com", Uuid::now_v7());
        let user_id = client
            .create_user(&test_email)
            .await
            .expect("Failed to create user");

        client
            .add_org_membership(tenant_id, user_id, UserRole::Owner, None)
            .await
            .expect("Failed to add membership");

        let membership = client
            .get_org_membership(tenant_id, user_id)
            .await
            .expect("Failed to get membership")
            .expect("Membership should exist");

        assert_eq!(membership.tenant_id, tenant_id);
        assert_eq!(membership.user_id, user_id);
        assert_eq!(membership.role, UserRole::Owner);

        // Cleanup
        client
            .hard_delete_org_membership(tenant_id, user_id)
            .await
            .ok();
        sqlx::query("DELETE FROM users WHERE user_id = $1")
            .bind(user_id)
            .execute(client.pool())
            .await
            .ok();
        sqlx::query("DELETE FROM tenants WHERE tenant_id = $1")
            .bind(tenant_id)
            .execute(client.pool())
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_user_orgs() {
        let client = create_test_client().await;
        let tenant_id_1 = create_test_tenant(&client).await;
        let tenant_id_2 = create_test_tenant(&client).await;
        let test_email = format!("test-{}@example.com", Uuid::now_v7());
        let user_id = client
            .create_user(&test_email)
            .await
            .expect("Failed to create user");

        client
            .add_org_membership(tenant_id_1, user_id, UserRole::Owner, None)
            .await
            .expect("Failed to add membership 1");
        client
            .add_org_membership(tenant_id_2, user_id, UserRole::Member, None)
            .await
            .expect("Failed to add membership 2");

        let orgs = client
            .get_user_orgs(user_id)
            .await
            .expect("Failed to get user orgs");

        assert_eq!(orgs.len(), 2);

        // Cleanup
        client
            .hard_delete_org_membership(tenant_id_1, user_id)
            .await
            .ok();
        client
            .hard_delete_org_membership(tenant_id_2, user_id)
            .await
            .ok();
        sqlx::query("DELETE FROM users WHERE user_id = $1")
            .bind(user_id)
            .execute(client.pool())
            .await
            .ok();
        sqlx::query("DELETE FROM tenants WHERE tenant_id = $1")
            .bind(tenant_id_1)
            .execute(client.pool())
            .await
            .ok();
        sqlx::query("DELETE FROM tenants WHERE tenant_id = $1")
            .bind(tenant_id_2)
            .execute(client.pool())
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_org_members() {
        let client = create_test_client().await;
        let tenant_id = create_test_tenant(&client).await;
        let test_email_1 = format!("test-{}@example.com", Uuid::now_v7());
        let test_email_2 = format!("test-{}@example.com", Uuid::now_v7());
        let user_id_1 = client
            .create_user(&test_email_1)
            .await
            .expect("Failed to create user 1");
        let user_id_2 = client
            .create_user(&test_email_2)
            .await
            .expect("Failed to create user 2");

        client
            .add_org_membership(tenant_id, user_id_1, UserRole::Owner, None)
            .await
            .expect("Failed to add member 1");
        client
            .add_org_membership(tenant_id, user_id_2, UserRole::Member, Some(user_id_1))
            .await
            .expect("Failed to add member 2");

        let members = client
            .get_org_members(tenant_id)
            .await
            .expect("Failed to get org members");

        assert_eq!(members.len(), 2);

        // Cleanup
        client
            .hard_delete_org_membership(tenant_id, user_id_1)
            .await
            .ok();
        client
            .hard_delete_org_membership(tenant_id, user_id_2)
            .await
            .ok();
        sqlx::query("DELETE FROM users WHERE user_id = $1")
            .bind(user_id_1)
            .execute(client.pool())
            .await
            .ok();
        sqlx::query("DELETE FROM users WHERE user_id = $1")
            .bind(user_id_2)
            .execute(client.pool())
            .await
            .ok();
        sqlx::query("DELETE FROM tenants WHERE tenant_id = $1")
            .bind(tenant_id)
            .execute(client.pool())
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_create_and_get_invitation() {
        let client = create_test_client().await;
        let tenant_id = create_test_tenant(&client).await;
        let test_email = format!("test-{}@example.com", Uuid::now_v7());
        let invited_by = client
            .create_user(&test_email)
            .await
            .expect("Failed to create inviter");

        let token_hash = format!("test-invite-{}", Uuid::now_v7());
        let email = "invitee@example.com";
        let expires_at = Utc::now() + chrono::Duration::days(7);

        client
            .create_invitation(&CreateInvitationParams {
                token_hash: &token_hash,
                tenant_id,
                email,
                role: UserRole::Member,
                invited_by,
                expires_at,
            })
            .await
            .expect("Failed to create invitation");

        let invitation = client
            .get_invitation(&token_hash)
            .await
            .expect("Failed to get invitation")
            .expect("Invitation should exist");

        assert_eq!(invitation.token, token_hash);
        assert_eq!(invitation.tenant_id, tenant_id);
        assert_eq!(invitation.email, email);
        assert_eq!(invitation.role, UserRole::Member);
        assert!(!invitation.accepted);
        assert!(invitation.is_valid());

        // Cleanup
        sqlx::query("DELETE FROM org_invitations WHERE token_hash = $1")
            .bind(&token_hash)
            .execute(client.pool())
            .await
            .ok();
        sqlx::query("DELETE FROM users WHERE user_id = $1")
            .bind(invited_by)
            .execute(client.pool())
            .await
            .ok();
        sqlx::query("DELETE FROM tenants WHERE tenant_id = $1")
            .bind(tenant_id)
            .execute(client.pool())
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_mark_invitation_accepted() {
        let client = create_test_client().await;
        let tenant_id = create_test_tenant(&client).await;
        let test_email = format!("test-{}@example.com", Uuid::now_v7());
        let invited_by = client
            .create_user(&test_email)
            .await
            .expect("Failed to create inviter");

        let token_hash = format!("test-invite-{}", Uuid::now_v7());
        let expires_at = Utc::now() + chrono::Duration::days(7);

        client
            .create_invitation(&CreateInvitationParams {
                token_hash: &token_hash,
                tenant_id,
                email: "test@example.com",
                role: UserRole::Member,
                invited_by,
                expires_at,
            })
            .await
            .expect("Failed to create invitation");

        client
            .mark_invitation_accepted(&token_hash)
            .await
            .expect("Failed to mark invitation accepted");

        let invitation = client
            .get_invitation(&token_hash)
            .await
            .expect("Failed to get invitation")
            .expect("Invitation should exist");
        assert!(invitation.accepted);
        assert!(!invitation.is_valid());

        // Cleanup
        sqlx::query("DELETE FROM org_invitations WHERE token_hash = $1")
            .bind(&token_hash)
            .execute(client.pool())
            .await
            .ok();
        sqlx::query("DELETE FROM users WHERE user_id = $1")
            .bind(invited_by)
            .execute(client.pool())
            .await
            .ok();
        sqlx::query("DELETE FROM tenants WHERE tenant_id = $1")
            .bind(tenant_id)
            .execute(client.pool())
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_create_and_get_tenant() {
        let client = create_test_client().await;
        let tenant_id = Uuid::now_v7();
        let name = "Test Organization";

        client
            .create_tenant(tenant_id, name)
            .await
            .expect("Failed to create tenant");

        let tenant_name = client
            .get_tenant_name(tenant_id)
            .await
            .expect("Failed to get tenant")
            .expect("Tenant should exist");

        assert_eq!(tenant_name, name);

        // Cleanup
        sqlx::query("DELETE FROM tenants WHERE tenant_id = $1")
            .bind(tenant_id)
            .execute(client.pool())
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_users_by_ids_batch() {
        let client = create_test_client().await;
        let email_1 = format!("test-{}@example.com", Uuid::now_v7());
        let email_2 = format!("test-{}@example.com", Uuid::now_v7());

        let user_id_1 = client
            .create_user(&email_1)
            .await
            .expect("Failed to create user 1");
        let user_id_2 = client
            .create_user(&email_2)
            .await
            .expect("Failed to create user 2");

        let users = client
            .get_users_by_ids(vec![user_id_1, user_id_2])
            .await
            .expect("Failed to batch-fetch users");

        assert_eq!(users.len(), 2);

        let empty = client
            .get_users_by_ids(vec![])
            .await
            .expect("Failed on empty list");
        assert!(empty.is_empty());

        // Cleanup
        sqlx::query("DELETE FROM users WHERE user_id = $1")
            .bind(user_id_1)
            .execute(client.pool())
            .await
            .ok();
        sqlx::query("DELETE FROM users WHERE user_id = $1")
            .bind(user_id_2)
            .execute(client.pool())
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_delete_user_sessions_bulk() {
        let client = create_test_client().await;
        let tenant_id = create_test_tenant(&client).await;
        let test_email = format!("test-{}@example.com", Uuid::now_v7());
        let user_id = client
            .create_user(&test_email)
            .await
            .expect("Failed to create user");

        let expires_at = Utc::now() + chrono::Duration::days(1);

        let hash_1 = format!("test-session-{}", Uuid::now_v7());
        let hash_2 = format!("test-session-{}", Uuid::now_v7());

        for hash in [&hash_1, &hash_2] {
            client
                .create_session(&CreateSessionParams {
                    session_id_hash: hash,
                    user_id,
                    tenant_id,
                    expires_at,
                    user_agent: None,
                    ip_address: None,
                })
                .await
                .expect("Failed to create session");
        }

        client
            .delete_user_sessions(user_id)
            .await
            .expect("Failed to delete user sessions");

        assert!(client.get_session(&hash_1).await.unwrap().is_none());
        assert!(client.get_session(&hash_2).await.unwrap().is_none());

        // Cleanup
        sqlx::query("DELETE FROM users WHERE user_id = $1")
            .bind(user_id)
            .execute(client.pool())
            .await
            .ok();
        sqlx::query("DELETE FROM tenants WHERE tenant_id = $1")
            .bind(tenant_id)
            .execute(client.pool())
            .await
            .ok();
    }
}
