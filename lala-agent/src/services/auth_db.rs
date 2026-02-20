// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

//! Database operations for authentication.
//!
//! All queries target the system keyspace since auth data is global across tenants.

use crate::models::auth::{
    MagicLinkToken, OrgInvitation, OrgMembership, Session, User, UserRole, UserStatus,
};
use scylla::frame::value::CqlTimestamp;
use scylla::transport::errors::QueryError;
use scylla::Session as ScyllaSession;
use std::sync::Arc;
use uuid::Uuid;

/// Parameters for creating a session.
pub struct CreateSessionParams<'a> {
    pub session_id_hash: &'a str,
    pub user_id: Uuid,
    pub tenant_id: &'a str,
    pub expires_at: i64,
    pub user_agent: Option<&'a str>,
    pub ip_address: Option<&'a str>,
}

/// Parameters for creating a magic link token.
pub struct CreateMagicLinkParams<'a> {
    pub token_hash: &'a str,
    pub email: &'a str,
    pub tenant_id: Option<&'a str>,
    pub redirect_url: Option<&'a str>,
    pub expires_at: i64,
}

/// Parameters for creating an invitation.
pub struct CreateInvitationParams<'a> {
    pub token_hash: &'a str,
    pub tenant_id: &'a str,
    pub email: &'a str,
    pub role: UserRole,
    pub invited_by: Uuid,
    pub expires_at: i64,
}

/// Authentication database client.
///
/// Wraps a Cassandra session and provides auth-specific operations.
/// All operations target the system keyspace.
#[derive(Clone)]
pub struct AuthDbClient {
    pub session: Arc<ScyllaSession>,
    pub keyspace: String,
}

impl AuthDbClient {
    /// Create a new auth database client.
    pub fn new(session: Arc<ScyllaSession>, keyspace: String) -> Self {
        Self { session, keyspace }
    }

    // ========== User Operations ==========

    /// Get a user by email.
    pub async fn get_user_by_email(&self, email: &str) -> Result<Option<User>, QueryError> {
        let query = format!(
            "SELECT user_id, email, email_verified, created_at, updated_at, last_login_at, status
             FROM {}.users WHERE email = ? ALLOW FILTERING",
            self.keyspace
        );

        let result = self.session.query_unpaged(query, (email,)).await?;
        let rows_result = match result.into_rows_result() {
            Ok(r) => r,
            Err(_) => return Ok(None),
        };

        self.parse_user_row(rows_result)
    }

    /// Get a user by ID.
    pub async fn get_user_by_id(&self, user_id: Uuid) -> Result<Option<User>, QueryError> {
        let query = format!(
            "SELECT user_id, email, email_verified, created_at, updated_at, last_login_at, status
             FROM {}.users WHERE user_id = ?",
            self.keyspace
        );

        let result = self.session.query_unpaged(query, (user_id,)).await?;
        let rows_result = match result.into_rows_result() {
            Ok(r) => r,
            Err(_) => return Ok(None),
        };

        self.parse_user_row(rows_result)
    }

    /// Get multiple users by their IDs in a single query.
    pub async fn get_users_by_ids(&self, user_ids: Vec<Uuid>) -> Result<Vec<User>, QueryError> {
        if user_ids.is_empty() {
            return Ok(Vec::new());
        }

        let query = format!(
            "SELECT user_id, email, email_verified, created_at, updated_at, last_login_at, status
             FROM {}.users WHERE user_id IN ?",
            self.keyspace
        );

        let result = self.session.query_unpaged(query, (user_ids,)).await?;
        let rows_result = match result.into_rows_result() {
            Ok(r) => r,
            Err(_) => return Ok(Vec::new()),
        };

        let rows = rows_result
            .rows::<(
                Uuid,
                String,
                bool,
                CqlTimestamp,
                CqlTimestamp,
                Option<CqlTimestamp>,
                String,
            )>()
            .map_err(|e| {
                QueryError::DbError(
                    scylla::transport::errors::DbError::Other(0),
                    format!("Failed to deserialize users: {}", e),
                )
            })?;

        let mut users = Vec::new();
        for row_result in rows {
            let (user_id, email, email_verified, created_at, updated_at, last_login_at, status) =
                row_result.map_err(|e| {
                    QueryError::DbError(
                        scylla::transport::errors::DbError::Other(0),
                        format!("Failed to parse user row: {}", e),
                    )
                })?;

            users.push(User {
                user_id,
                email,
                email_verified,
                created_at: created_at.0,
                updated_at: updated_at.0,
                last_login_at: last_login_at.map(|t| t.0),
                status: UserStatus::parse(&status).unwrap_or(UserStatus::Active),
            });
        }

        Ok(users)
    }

    /// Create a new user.
    pub async fn create_user(&self, email: &str) -> Result<Uuid, QueryError> {
        let user_id = Uuid::now_v7();
        let now = chrono::Utc::now().timestamp_millis();

        let query = format!(
            "INSERT INTO {}.users (user_id, email, email_verified, created_at, updated_at, status)
             VALUES (?, ?, false, ?, ?, 'active')",
            self.keyspace
        );

        self.session
            .query_unpaged(
                query,
                (user_id, email, CqlTimestamp(now), CqlTimestamp(now)),
            )
            .await?;

        Ok(user_id)
    }

    /// Update user's last login time.
    pub async fn update_user_last_login(&self, user_id: Uuid) -> Result<(), QueryError> {
        let now = chrono::Utc::now().timestamp_millis();

        let query = format!(
            "UPDATE {}.users SET last_login_at = ?, updated_at = ? WHERE user_id = ?",
            self.keyspace
        );

        self.session
            .query_unpaged(query, (CqlTimestamp(now), CqlTimestamp(now), user_id))
            .await?;

        Ok(())
    }

    /// Mark user's email as verified.
    pub async fn set_user_email_verified(&self, user_id: Uuid) -> Result<(), QueryError> {
        let now = chrono::Utc::now().timestamp_millis();

        let query = format!(
            "UPDATE {}.users SET email_verified = true, updated_at = ? WHERE user_id = ?",
            self.keyspace
        );

        self.session
            .query_unpaged(query, (CqlTimestamp(now), user_id))
            .await?;

        Ok(())
    }

    fn parse_user_row(
        &self,
        rows_result: scylla::QueryRowsResult,
    ) -> Result<Option<User>, QueryError> {
        let rows = rows_result
            .rows::<(
                Uuid,
                String,
                bool,
                CqlTimestamp,
                CqlTimestamp,
                Option<CqlTimestamp>,
                String,
            )>()
            .map_err(|e| {
                QueryError::DbError(
                    scylla::transport::errors::DbError::Other(0),
                    format!("Failed to deserialize user: {}", e),
                )
            })?;

        let Some(row_result) = rows.into_iter().next() else {
            return Ok(None);
        };

        let (user_id, email, email_verified, created_at, updated_at, last_login_at, status) =
            row_result.map_err(|e| {
                QueryError::DbError(
                    scylla::transport::errors::DbError::Other(0),
                    format!("Failed to parse user row: {}", e),
                )
            })?;

        Ok(Some(User {
            user_id,
            email,
            email_verified,
            created_at: created_at.0,
            updated_at: updated_at.0,
            last_login_at: last_login_at.map(|t| t.0),
            status: UserStatus::parse(&status).unwrap_or(UserStatus::Active),
        }))
    }

    // ========== Session Operations ==========

    /// Create a new session.
    pub async fn create_session(&self, params: &CreateSessionParams<'_>) -> Result<(), QueryError> {
        let now = chrono::Utc::now().timestamp_millis();

        let query = format!(
            "INSERT INTO {}.sessions
             (session_id, user_id, tenant_id, created_at, expires_at, last_active_at, user_agent, ip_address)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            self.keyspace
        );

        self.session
            .query_unpaged(
                query,
                (
                    params.session_id_hash,
                    params.user_id,
                    params.tenant_id,
                    CqlTimestamp(now),
                    CqlTimestamp(params.expires_at),
                    CqlTimestamp(now),
                    params.user_agent,
                    params.ip_address,
                ),
            )
            .await?;

        Ok(())
    }

    /// Get a session by its hash.
    pub async fn get_session(&self, session_id_hash: &str) -> Result<Option<Session>, QueryError> {
        let query = format!(
            "SELECT session_id, user_id, tenant_id, created_at, expires_at, last_active_at, user_agent, ip_address
             FROM {}.sessions WHERE session_id = ?",
            self.keyspace
        );

        let result = self
            .session
            .query_unpaged(query, (session_id_hash,))
            .await?;
        let rows_result = match result.into_rows_result() {
            Ok(r) => r,
            Err(_) => return Ok(None),
        };

        let rows = rows_result
            .rows::<(
                String,
                Uuid,
                String,
                CqlTimestamp,
                CqlTimestamp,
                CqlTimestamp,
                Option<String>,
                Option<String>,
            )>()
            .map_err(|e| {
                QueryError::DbError(
                    scylla::transport::errors::DbError::Other(0),
                    format!("Failed to deserialize session: {}", e),
                )
            })?;

        let Some(row_result) = rows.into_iter().next() else {
            return Ok(None);
        };

        let (
            session_id,
            user_id,
            tenant_id,
            created_at,
            expires_at,
            last_active_at,
            user_agent,
            ip_address,
        ) = row_result.map_err(|e| {
            QueryError::DbError(
                scylla::transport::errors::DbError::Other(0),
                format!("Failed to parse session row: {}", e),
            )
        })?;

        Ok(Some(Session {
            session_id,
            user_id,
            tenant_id,
            created_at: created_at.0,
            expires_at: expires_at.0,
            last_active_at: last_active_at.0,
            user_agent,
            ip_address,
        }))
    }

    /// Delete a session.
    pub async fn delete_session(&self, session_id_hash: &str) -> Result<(), QueryError> {
        let query = format!(
            "DELETE FROM {}.sessions WHERE session_id = ?",
            self.keyspace
        );

        self.session
            .query_unpaged(query, (session_id_hash,))
            .await?;

        Ok(())
    }

    /// Delete all sessions for a user.
    pub async fn delete_user_sessions(&self, user_id: Uuid) -> Result<(), QueryError> {
        let query = format!(
            "SELECT session_id FROM {}.sessions WHERE user_id = ? ALLOW FILTERING",
            self.keyspace
        );

        let result = self.session.query_unpaged(query, (user_id,)).await?;
        let rows_result = match result.into_rows_result() {
            Ok(r) => r,
            Err(_) => return Ok(()),
        };

        let rows = rows_result.rows::<(String,)>().map_err(|e| {
            QueryError::DbError(
                scylla::transport::errors::DbError::Other(0),
                format!("Failed to deserialize session IDs: {}", e),
            )
        })?;

        for row_result in rows {
            let (session_id,) = row_result.map_err(|e| {
                QueryError::DbError(
                    scylla::transport::errors::DbError::Other(0),
                    format!("Failed to parse session ID: {}", e),
                )
            })?;
            self.delete_session(&session_id).await?;
        }

        Ok(())
    }

    /// Update session's last active time.
    pub async fn touch_session(&self, session_id_hash: &str) -> Result<(), QueryError> {
        let now = chrono::Utc::now().timestamp_millis();

        let query = format!(
            "UPDATE {}.sessions SET last_active_at = ? WHERE session_id = ?",
            self.keyspace
        );

        self.session
            .query_unpaged(query, (CqlTimestamp(now), session_id_hash))
            .await?;

        Ok(())
    }

    // ========== Magic Link Token Operations ==========

    /// Create a magic link token.
    pub async fn create_magic_link_token(
        &self,
        params: &CreateMagicLinkParams<'_>,
    ) -> Result<(), QueryError> {
        let now = chrono::Utc::now().timestamp_millis();

        let query = format!(
            "INSERT INTO {}.magic_link_tokens
             (token_hash, email, tenant_id, redirect_url, created_at, expires_at, used)
             VALUES (?, ?, ?, ?, ?, ?, false)",
            self.keyspace
        );

        self.session
            .query_unpaged(
                query,
                (
                    params.token_hash,
                    params.email,
                    params.tenant_id,
                    params.redirect_url,
                    CqlTimestamp(now),
                    CqlTimestamp(params.expires_at),
                ),
            )
            .await?;

        Ok(())
    }

    /// Get a magic link token.
    pub async fn get_magic_link_token(
        &self,
        token_hash: &str,
    ) -> Result<Option<MagicLinkToken>, QueryError> {
        let query = format!(
            "SELECT token_hash, email, tenant_id, redirect_url, created_at, expires_at, used
             FROM {}.magic_link_tokens WHERE token_hash = ?",
            self.keyspace
        );

        let result = self.session.query_unpaged(query, (token_hash,)).await?;
        let rows_result = match result.into_rows_result() {
            Ok(r) => r,
            Err(_) => return Ok(None),
        };

        let rows = rows_result
            .rows::<(
                String,
                String,
                Option<String>,
                Option<String>,
                CqlTimestamp,
                CqlTimestamp,
                bool,
            )>()
            .map_err(|e| {
                QueryError::DbError(
                    scylla::transport::errors::DbError::Other(0),
                    format!("Failed to deserialize magic link token: {}", e),
                )
            })?;

        let Some(row_result) = rows.into_iter().next() else {
            return Ok(None);
        };

        let (token, email, tenant_id, redirect_url, created_at, expires_at, used) = row_result
            .map_err(|e| {
                QueryError::DbError(
                    scylla::transport::errors::DbError::Other(0),
                    format!("Failed to parse magic link token row: {}", e),
                )
            })?;

        Ok(Some(MagicLinkToken {
            token,
            email,
            tenant_id,
            redirect_url,
            created_at: created_at.0,
            expires_at: expires_at.0,
            used,
        }))
    }

    /// Mark a magic link token as used.
    pub async fn mark_magic_link_used(&self, token_hash: &str) -> Result<(), QueryError> {
        let query = format!(
            "UPDATE {}.magic_link_tokens SET used = true WHERE token_hash = ?",
            self.keyspace
        );

        self.session.query_unpaged(query, (token_hash,)).await?;

        Ok(())
    }

    // ========== Organization Membership Operations ==========

    /// Add a user to an organization.
    pub async fn add_org_membership(
        &self,
        tenant_id: &str,
        user_id: Uuid,
        role: UserRole,
        invited_by: Option<Uuid>,
    ) -> Result<(), QueryError> {
        let now = chrono::Utc::now().timestamp_millis();

        let query = format!(
            "INSERT INTO {}.org_memberships (tenant_id, user_id, role, joined_at, invited_by)
             VALUES (?, ?, ?, ?, ?)",
            self.keyspace
        );

        self.session
            .query_unpaged(
                query,
                (
                    tenant_id,
                    user_id,
                    role.as_str(),
                    CqlTimestamp(now),
                    invited_by,
                ),
            )
            .await?;

        let query = format!(
            "INSERT INTO {}.user_orgs (user_id, tenant_id, role, joined_at)
             VALUES (?, ?, ?, ?)",
            self.keyspace
        );

        self.session
            .query_unpaged(
                query,
                (user_id, tenant_id, role.as_str(), CqlTimestamp(now)),
            )
            .await?;

        Ok(())
    }

    /// Get a user's membership in an organization.
    pub async fn get_org_membership(
        &self,
        tenant_id: &str,
        user_id: Uuid,
    ) -> Result<Option<OrgMembership>, QueryError> {
        let query = format!(
            "SELECT tenant_id, user_id, role, joined_at, invited_by
             FROM {}.org_memberships WHERE tenant_id = ? AND user_id = ?",
            self.keyspace
        );

        let result = self
            .session
            .query_unpaged(query, (tenant_id, user_id))
            .await?;
        let rows_result = match result.into_rows_result() {
            Ok(r) => r,
            Err(_) => return Ok(None),
        };

        let rows = rows_result
            .rows::<(String, Uuid, String, CqlTimestamp, Option<Uuid>)>()
            .map_err(|e| {
                QueryError::DbError(
                    scylla::transport::errors::DbError::Other(0),
                    format!("Failed to deserialize org membership: {}", e),
                )
            })?;

        let Some(row_result) = rows.into_iter().next() else {
            return Ok(None);
        };

        let (tenant_id, user_id, role, joined_at, invited_by) = row_result.map_err(|e| {
            QueryError::DbError(
                scylla::transport::errors::DbError::Other(0),
                format!("Failed to parse org membership row: {}", e),
            )
        })?;

        Ok(Some(OrgMembership {
            tenant_id,
            user_id,
            role: UserRole::parse(&role).unwrap_or(UserRole::Member),
            joined_at: joined_at.0,
            invited_by,
        }))
    }

    /// Get all organizations a user belongs to.
    pub async fn get_user_orgs(&self, user_id: Uuid) -> Result<Vec<OrgMembership>, QueryError> {
        let query = format!(
            "SELECT tenant_id, role, joined_at FROM {}.user_orgs WHERE user_id = ?",
            self.keyspace
        );

        let result = self.session.query_unpaged(query, (user_id,)).await?;
        let rows_result = match result.into_rows_result() {
            Ok(r) => r,
            Err(_) => return Ok(Vec::new()),
        };

        let rows = rows_result
            .rows::<(String, String, CqlTimestamp)>()
            .map_err(|e| {
                QueryError::DbError(
                    scylla::transport::errors::DbError::Other(0),
                    format!("Failed to deserialize user orgs: {}", e),
                )
            })?;

        let mut memberships = Vec::new();
        for row_result in rows {
            let (tenant_id, role, joined_at) = row_result.map_err(|e| {
                QueryError::DbError(
                    scylla::transport::errors::DbError::Other(0),
                    format!("Failed to parse user org row: {}", e),
                )
            })?;

            memberships.push(OrgMembership {
                tenant_id,
                user_id,
                role: UserRole::parse(&role).unwrap_or(UserRole::Member),
                joined_at: joined_at.0,
                invited_by: None,
            });
        }

        Ok(memberships)
    }

    /// Get all members of an organization.
    pub async fn get_org_members(&self, tenant_id: &str) -> Result<Vec<OrgMembership>, QueryError> {
        let query = format!(
            "SELECT tenant_id, user_id, role, joined_at, invited_by
             FROM {}.org_memberships WHERE tenant_id = ?",
            self.keyspace
        );

        let result = self.session.query_unpaged(query, (tenant_id,)).await?;
        let rows_result = match result.into_rows_result() {
            Ok(r) => r,
            Err(_) => return Ok(Vec::new()),
        };

        let rows = rows_result
            .rows::<(String, Uuid, String, CqlTimestamp, Option<Uuid>)>()
            .map_err(|e| {
                QueryError::DbError(
                    scylla::transport::errors::DbError::Other(0),
                    format!("Failed to deserialize org members: {}", e),
                )
            })?;

        let mut members = Vec::new();
        for row_result in rows {
            let (tenant_id, user_id, role, joined_at, invited_by) = row_result.map_err(|e| {
                QueryError::DbError(
                    scylla::transport::errors::DbError::Other(0),
                    format!("Failed to parse org member row: {}", e),
                )
            })?;

            members.push(OrgMembership {
                tenant_id,
                user_id,
                role: UserRole::parse(&role).unwrap_or(UserRole::Member),
                joined_at: joined_at.0,
                invited_by,
            });
        }

        Ok(members)
    }

    /// Remove a user from an organization.
    pub async fn remove_org_membership(
        &self,
        tenant_id: &str,
        user_id: Uuid,
    ) -> Result<(), QueryError> {
        let query = format!(
            "DELETE FROM {}.org_memberships WHERE tenant_id = ? AND user_id = ?",
            self.keyspace
        );
        self.session
            .query_unpaged(query, (tenant_id, user_id))
            .await?;

        let query = format!(
            "DELETE FROM {}.user_orgs WHERE user_id = ? AND tenant_id = ?",
            self.keyspace
        );
        self.session
            .query_unpaged(query, (user_id, tenant_id))
            .await?;

        Ok(())
    }

    // ========== Organization Invitation Operations ==========

    /// Create an organization invitation.
    pub async fn create_invitation(
        &self,
        params: &CreateInvitationParams<'_>,
    ) -> Result<(), QueryError> {
        let now = chrono::Utc::now().timestamp_millis();

        let query = format!(
            "INSERT INTO {}.org_invitations
             (token_hash, tenant_id, email, role, invited_by, created_at, expires_at, accepted)
             VALUES (?, ?, ?, ?, ?, ?, ?, false)",
            self.keyspace
        );

        self.session
            .query_unpaged(
                query,
                (
                    params.token_hash,
                    params.tenant_id,
                    params.email,
                    params.role.as_str(),
                    params.invited_by,
                    CqlTimestamp(now),
                    CqlTimestamp(params.expires_at),
                ),
            )
            .await?;

        Ok(())
    }

    /// Get an invitation by token.
    pub async fn get_invitation(
        &self,
        token_hash: &str,
    ) -> Result<Option<OrgInvitation>, QueryError> {
        let query = format!(
            "SELECT token_hash, tenant_id, email, role, invited_by, created_at, expires_at, accepted
             FROM {}.org_invitations WHERE token_hash = ?",
            self.keyspace
        );

        let result = self.session.query_unpaged(query, (token_hash,)).await?;
        let rows_result = match result.into_rows_result() {
            Ok(r) => r,
            Err(_) => return Ok(None),
        };

        let rows = rows_result
            .rows::<(
                String,
                String,
                String,
                String,
                Uuid,
                CqlTimestamp,
                CqlTimestamp,
                bool,
            )>()
            .map_err(|e| {
                QueryError::DbError(
                    scylla::transport::errors::DbError::Other(0),
                    format!("Failed to deserialize invitation: {}", e),
                )
            })?;

        let Some(row_result) = rows.into_iter().next() else {
            return Ok(None);
        };

        let (token, tenant_id, email, role, invited_by, created_at, expires_at, accepted) =
            row_result.map_err(|e| {
                QueryError::DbError(
                    scylla::transport::errors::DbError::Other(0),
                    format!("Failed to parse invitation row: {}", e),
                )
            })?;

        Ok(Some(OrgInvitation {
            token,
            tenant_id,
            email,
            role: UserRole::parse(&role).unwrap_or(UserRole::Member),
            invited_by,
            created_at: created_at.0,
            expires_at: expires_at.0,
            accepted,
        }))
    }

    /// Mark an invitation as accepted.
    pub async fn mark_invitation_accepted(&self, token_hash: &str) -> Result<(), QueryError> {
        let query = format!(
            "UPDATE {}.org_invitations SET accepted = true WHERE token_hash = ?",
            self.keyspace
        );

        self.session.query_unpaged(query, (token_hash,)).await?;

        Ok(())
    }

    // ========== Tenant Operations ==========

    /// Create a new tenant.
    pub async fn create_tenant(&self, tenant_id: &str, name: &str) -> Result<(), QueryError> {
        let now = chrono::Utc::now().timestamp_millis();

        let query = format!(
            "INSERT INTO {}.tenants (tenant_id, name, created_at) VALUES (?, ?, ?)",
            self.keyspace
        );

        self.session
            .query_unpaged(query, (tenant_id, name, CqlTimestamp(now)))
            .await?;

        Ok(())
    }

    /// Get tenant name by ID.
    pub async fn get_tenant_name(&self, tenant_id: &str) -> Result<Option<String>, QueryError> {
        let query = format!(
            "SELECT name FROM {}.tenants WHERE tenant_id = ?",
            self.keyspace
        );

        let result = self.session.query_unpaged(query, (tenant_id,)).await?;
        let rows_result = match result.into_rows_result() {
            Ok(r) => r,
            Err(_) => return Ok(None),
        };

        let mut rows = rows_result.rows::<(String,)>().map_err(|e| {
            QueryError::DbError(
                scylla::transport::errors::DbError::Other(0),
                format!("Failed to deserialize tenant: {}", e),
            )
        })?;

        if let Some(row_result) = rows.next() {
            let (name,) = row_result.map_err(|e| {
                QueryError::DbError(
                    scylla::transport::errors::DbError::Other(0),
                    format!("Failed to parse tenant row: {}", e),
                )
            })?;
            return Ok(Some(name));
        }

        Ok(None)
    }
}

// Tests first - TDD style
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

    // ========== Integration Tests (require Cassandra) ==========

    /// Helper to create an AuthDbClient for tests.
    async fn create_test_client() -> AuthDbClient {
        use scylla::SessionBuilder;

        let hosts_str = std::env::var("CASSANDRA_HOSTS").expect("CASSANDRA_HOSTS must be set");
        let hosts: Vec<String> = hosts_str.split(',').map(|s| s.trim().to_string()).collect();
        let keyspace = std::env::var("CASSANDRA_SYSTEM_KEYSPACE")
            .expect("CASSANDRA_SYSTEM_KEYSPACE must be set");

        let session = SessionBuilder::new()
            .known_nodes(&hosts)
            .build()
            .await
            .expect("Failed to connect to Cassandra");

        AuthDbClient::new(Arc::new(session), keyspace)
    }

    #[tokio::test]
    #[ignore]
    async fn test_create_and_get_user_by_email() {
        let client = create_test_client().await;
        let test_email = format!("test-{}@example.com", Uuid::now_v7());

        // Create user
        let user_id = client
            .create_user(&test_email)
            .await
            .expect("Failed to create user");

        // Get user by email
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
        let query = format!("DELETE FROM {}.users WHERE user_id = ?", client.keyspace);
        client.session.query_unpaged(query, (user_id,)).await.ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_create_and_get_user_by_id() {
        let client = create_test_client().await;
        let test_email = format!("test-{}@example.com", Uuid::now_v7());

        // Create user
        let user_id = client
            .create_user(&test_email)
            .await
            .expect("Failed to create user");

        // Get user by ID
        let user = client
            .get_user_by_id(user_id)
            .await
            .expect("Failed to get user")
            .expect("User should exist");

        assert_eq!(user.user_id, user_id);
        assert_eq!(user.email, test_email);

        // Cleanup
        let query = format!("DELETE FROM {}.users WHERE user_id = ?", client.keyspace);
        client.session.query_unpaged(query, (user_id,)).await.ok();
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

        // Create user
        let user_id = client
            .create_user(&test_email)
            .await
            .expect("Failed to create user");

        // Initially no last_login_at
        let user = client
            .get_user_by_id(user_id)
            .await
            .expect("Failed to get user")
            .expect("User should exist");
        assert!(user.last_login_at.is_none());

        // Update last login
        client
            .update_user_last_login(user_id)
            .await
            .expect("Failed to update last login");

        // Now last_login_at should be set
        let user = client
            .get_user_by_id(user_id)
            .await
            .expect("Failed to get user")
            .expect("User should exist");
        assert!(user.last_login_at.is_some());

        // Cleanup
        let query = format!("DELETE FROM {}.users WHERE user_id = ?", client.keyspace);
        client.session.query_unpaged(query, (user_id,)).await.ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_set_user_email_verified() {
        let client = create_test_client().await;
        let test_email = format!("test-{}@example.com", Uuid::now_v7());

        // Create user (email_verified = false by default)
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

        // Set email verified
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
        let query = format!("DELETE FROM {}.users WHERE user_id = ?", client.keyspace);
        client.session.query_unpaged(query, (user_id,)).await.ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_create_and_get_session() {
        let client = create_test_client().await;
        let session_hash = format!("test-session-{}", Uuid::now_v7());
        let user_id = Uuid::now_v7();
        let tenant_id = "default";
        let expires_at = chrono::Utc::now().timestamp_millis() + 86400000; // 1 day

        // Create session
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

        // Get session
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
    }

    #[tokio::test]
    #[ignore]
    async fn test_session_expiry_check() {
        let client = create_test_client().await;
        let session_hash = format!("test-session-{}", Uuid::now_v7());
        let user_id = Uuid::now_v7();
        let tenant_id = "default";
        let expires_at = chrono::Utc::now().timestamp_millis() - 1000; // Already expired

        // Create expired session
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

        // Get session and check expiry
        let session = client
            .get_session(&session_hash)
            .await
            .expect("Failed to get session")
            .expect("Session should exist");

        assert!(session.is_expired());

        // Cleanup
        client.delete_session(&session_hash).await.ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_delete_session() {
        let client = create_test_client().await;
        let session_hash = format!("test-session-{}", Uuid::now_v7());
        let user_id = Uuid::now_v7();
        let expires_at = chrono::Utc::now().timestamp_millis() + 86400000;

        // Create session
        client
            .create_session(&CreateSessionParams {
                session_id_hash: &session_hash,
                user_id,
                tenant_id: "default",
                expires_at,
                user_agent: None,
                ip_address: None,
            })
            .await
            .expect("Failed to create session");

        // Delete session
        client
            .delete_session(&session_hash)
            .await
            .expect("Failed to delete session");

        // Session should no longer exist
        let result = client
            .get_session(&session_hash)
            .await
            .expect("Query should succeed");
        assert!(result.is_none());
    }

    #[tokio::test]
    #[ignore]
    async fn test_create_and_get_magic_link_token() {
        let client = create_test_client().await;
        let token_hash = format!("test-token-{}", Uuid::now_v7());
        let email = "test@example.com";
        let expires_at = chrono::Utc::now().timestamp_millis() + 900000; // 15 min

        // Create token
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

        // Get token
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
        let query = format!(
            "DELETE FROM {}.magic_link_tokens WHERE token_hash = ?",
            client.keyspace
        );
        client
            .session
            .query_unpaged(query, (&token_hash,))
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_mark_magic_link_used() {
        let client = create_test_client().await;
        let token_hash = format!("test-token-{}", Uuid::now_v7());
        let expires_at = chrono::Utc::now().timestamp_millis() + 900000;

        // Create token
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

        // Mark as used
        client
            .mark_magic_link_used(&token_hash)
            .await
            .expect("Failed to mark token as used");

        // Token should now be invalid (used)
        let token = client
            .get_magic_link_token(&token_hash)
            .await
            .expect("Failed to get token")
            .expect("Token should exist");
        assert!(token.used);
        assert!(!token.is_valid());

        // Cleanup
        let query = format!(
            "DELETE FROM {}.magic_link_tokens WHERE token_hash = ?",
            client.keyspace
        );
        client
            .session
            .query_unpaged(query, (&token_hash,))
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_add_and_get_org_membership() {
        let client = create_test_client().await;
        let tenant_id = format!("test-tenant-{}", Uuid::now_v7());
        let user_id = Uuid::now_v7();

        // Add membership
        client
            .add_org_membership(&tenant_id, user_id, UserRole::Owner, None)
            .await
            .expect("Failed to add membership");

        // Get membership
        let membership = client
            .get_org_membership(&tenant_id, user_id)
            .await
            .expect("Failed to get membership")
            .expect("Membership should exist");

        assert_eq!(membership.tenant_id, tenant_id);
        assert_eq!(membership.user_id, user_id);
        assert_eq!(membership.role, UserRole::Owner);

        // Cleanup
        client.remove_org_membership(&tenant_id, user_id).await.ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_user_orgs() {
        let client = create_test_client().await;
        let tenant_id_1 = format!("test-tenant-{}", Uuid::now_v7());
        let tenant_id_2 = format!("test-tenant-{}", Uuid::now_v7());
        let user_id = Uuid::now_v7();

        // Add memberships to two orgs
        client
            .add_org_membership(&tenant_id_1, user_id, UserRole::Owner, None)
            .await
            .expect("Failed to add membership 1");
        client
            .add_org_membership(&tenant_id_2, user_id, UserRole::Member, None)
            .await
            .expect("Failed to add membership 2");

        // Get user's orgs
        let orgs = client
            .get_user_orgs(user_id)
            .await
            .expect("Failed to get user orgs");

        assert_eq!(orgs.len(), 2);

        // Cleanup
        client
            .remove_org_membership(&tenant_id_1, user_id)
            .await
            .ok();
        client
            .remove_org_membership(&tenant_id_2, user_id)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_org_members() {
        let client = create_test_client().await;
        let tenant_id = format!("test-tenant-{}", Uuid::now_v7());
        let user_id_1 = Uuid::now_v7();
        let user_id_2 = Uuid::now_v7();

        // Add two members
        client
            .add_org_membership(&tenant_id, user_id_1, UserRole::Owner, None)
            .await
            .expect("Failed to add member 1");
        client
            .add_org_membership(&tenant_id, user_id_2, UserRole::Member, Some(user_id_1))
            .await
            .expect("Failed to add member 2");

        // Get org members
        let members = client
            .get_org_members(&tenant_id)
            .await
            .expect("Failed to get org members");

        assert_eq!(members.len(), 2);

        // Cleanup
        client
            .remove_org_membership(&tenant_id, user_id_1)
            .await
            .ok();
        client
            .remove_org_membership(&tenant_id, user_id_2)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_create_and_get_invitation() {
        let client = create_test_client().await;
        let token_hash = format!("test-invite-{}", Uuid::now_v7());
        let tenant_id = "test-tenant";
        let email = "invitee@example.com";
        let invited_by = Uuid::now_v7();
        let expires_at = chrono::Utc::now().timestamp_millis() + 604800000; // 7 days

        // Create invitation
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

        // Get invitation
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
        let query = format!(
            "DELETE FROM {}.org_invitations WHERE token_hash = ?",
            client.keyspace
        );
        client
            .session
            .query_unpaged(query, (&token_hash,))
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_mark_invitation_accepted() {
        let client = create_test_client().await;
        let token_hash = format!("test-invite-{}", Uuid::now_v7());
        let expires_at = chrono::Utc::now().timestamp_millis() + 604800000;

        // Create invitation
        client
            .create_invitation(&CreateInvitationParams {
                token_hash: &token_hash,
                tenant_id: "tenant",
                email: "test@example.com",
                role: UserRole::Member,
                invited_by: Uuid::now_v7(),
                expires_at,
            })
            .await
            .expect("Failed to create invitation");

        // Mark as accepted
        client
            .mark_invitation_accepted(&token_hash)
            .await
            .expect("Failed to mark invitation accepted");

        // Check it's accepted
        let invitation = client
            .get_invitation(&token_hash)
            .await
            .expect("Failed to get invitation")
            .expect("Invitation should exist");
        assert!(invitation.accepted);
        assert!(!invitation.is_valid());

        // Cleanup
        let query = format!(
            "DELETE FROM {}.org_invitations WHERE token_hash = ?",
            client.keyspace
        );
        client
            .session
            .query_unpaged(query, (&token_hash,))
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_create_and_get_tenant() {
        let client = create_test_client().await;
        let tenant_id = format!("test-tenant-{}", Uuid::now_v7());
        let name = "Test Organization";

        // Create tenant
        client
            .create_tenant(&tenant_id, name)
            .await
            .expect("Failed to create tenant");

        // Get tenant name
        let tenant_name = client
            .get_tenant_name(&tenant_id)
            .await
            .expect("Failed to get tenant")
            .expect("Tenant should exist");

        assert_eq!(tenant_name, name);

        // Cleanup
        let query = format!(
            "DELETE FROM {}.tenants WHERE tenant_id = ?",
            client.keyspace
        );
        client
            .session
            .query_unpaged(query, (&tenant_id,))
            .await
            .ok();
    }
}
