// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

//! Authentication service for magic link and session management.

use crate::models::auth::{AuthUser, OrgMembership, User, UserRole};
use crate::services::auth_db::{
    AuthDbClient, CreateInvitationParams, CreateMagicLinkParams, CreateSessionParams,
};
use crate::services::email::EmailService;
use crate::services::logging::anonymize_email;
use anyhow::{anyhow, Context, Result};
use hex;
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::env;

/// Configuration for the auth service.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Session lifetime in days
    pub session_max_age_days: u64,
    /// Magic link expiry in minutes
    pub magic_link_expiry_minutes: u64,
    /// Invitation expiry in days
    pub invitation_expiry_days: u64,
}

impl AuthConfig {
    /// Load auth configuration from environment variables.
    pub fn from_env() -> Self {
        Self {
            session_max_age_days: env::var("SESSION_MAX_AGE_DAYS")
                .unwrap_or_else(|_| "365".to_string())
                .parse()
                .unwrap_or(365),
            magic_link_expiry_minutes: env::var("MAGIC_LINK_EXPIRY_MINUTES")
                .unwrap_or_else(|_| "15".to_string())
                .parse()
                .unwrap_or(15),
            invitation_expiry_days: env::var("INVITATION_EXPIRY_DAYS")
                .unwrap_or_else(|_| "7".to_string())
                .parse()
                .unwrap_or(7),
        }
    }
}

/// Authentication service.
pub struct AuthService {
    db: AuthDbClient,
    email: EmailService,
    config: AuthConfig,
}

/// Request to invite a user to an organization.
pub struct InviteRequest<'a> {
    pub tenant_id: &'a str,
    pub tenant_name: &'a str,
    pub email: &'a str,
    pub role: UserRole,
    pub inviter: &'a AuthUser,
}

impl AuthService {
    /// Create a new auth service.
    pub fn new(db: AuthDbClient, email: EmailService, config: AuthConfig) -> Self {
        Self { db, email, config }
    }

    // ========== Token Generation ==========

    /// Generate a secure random token.
    /// Returns (raw_token, hash) - raw_token is sent to user, hash is stored in DB.
    pub fn generate_token() -> (String, String) {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let raw_token = hex::encode(bytes);
        let hash = Self::hash_token(&raw_token);
        (raw_token, hash)
    }

    /// Hash a token for storage.
    pub fn hash_token(token: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        hex::encode(hasher.finalize())
    }

    // ========== Magic Link Flow ==========

    /// Request a magic link for authentication.
    /// Sends an email with a link to verify and create a session.
    pub async fn request_magic_link(&self, email: &str) -> Result<()> {
        let (raw_token, token_hash) = Self::generate_token();

        let expires_at = chrono::Utc::now().timestamp_millis()
            + (self.config.magic_link_expiry_minutes as i64 * 60 * 1000);

        self.db
            .create_magic_link_token(&CreateMagicLinkParams {
                token_hash: &token_hash,
                email,
                tenant_id: None,
                redirect_url: None,
                expires_at,
            })
            .await
            .context("Failed to create magic link token")?;

        self.email
            .send_magic_link(email, &raw_token)
            .await
            .context("Failed to send magic link email")?;

        Ok(())
    }

    /// Verify a magic link and create a session.
    /// Returns the session token and user info.
    pub async fn verify_magic_link(
        &self,
        token: &str,
        user_agent: Option<&str>,
        ip_address: Option<&str>,
        default_tenant_id: &str,
    ) -> Result<(String, User, String)> {
        let token_hash = Self::hash_token(token);

        // Get and validate token
        let magic_token = self
            .db
            .get_magic_link_token(&token_hash)
            .await
            .context("Failed to get magic link token")?
            .ok_or_else(|| anyhow!("Invalid or expired token"))?;

        if !magic_token.is_valid() {
            eprintln!(
                "[AUTH] Magic link verification failed for {}: token expired or already used",
                anonymize_email(&magic_token.email)
            );
            return Err(anyhow!("Token is expired or already used"));
        }

        // Mark token as used immediately to prevent reuse
        self.db
            .mark_magic_link_used(&token_hash)
            .await
            .context("Failed to mark token as used")?;

        // Get or create user
        let user = self
            .get_or_create_user(&magic_token.email, default_tenant_id)
            .await?;

        // Determine tenant to use for session
        let tenant_id = magic_token
            .tenant_id
            .as_deref()
            .unwrap_or(default_tenant_id);

        // Create session
        let session_token = self
            .create_user_session(user.user_id, tenant_id, user_agent, ip_address)
            .await?;

        println!(
            "[AUTH] User signed in via magic link: user_id={}, email={}, tenant={}{}",
            user.user_id,
            anonymize_email(&user.email),
            tenant_id,
            ip_address
                .map(|ip| format!(", ip={}", ip))
                .unwrap_or_default()
        );

        Ok((session_token, user, tenant_id.to_string()))
    }

    /// Get an existing user by email, or create a new one.
    async fn get_or_create_user(&self, email: &str, default_tenant_id: &str) -> Result<User> {
        match self
            .db
            .get_user_by_email(email)
            .await
            .context("Failed to get user")?
        {
            Some(user) => self.update_existing_user(user).await,
            None => self.create_new_user(email, default_tenant_id).await,
        }
    }

    /// Update an existing user's last login and email verification.
    async fn update_existing_user(&self, user: User) -> Result<User> {
        self.db
            .update_user_last_login(user.user_id)
            .await
            .context("Failed to update last login")?;

        if !user.email_verified {
            self.db
                .set_user_email_verified(user.user_id)
                .await
                .context("Failed to verify email")?;
        }

        self.db
            .get_user_by_id(user.user_id)
            .await
            .context("Failed to get updated user")?
            .ok_or_else(|| anyhow!("User disappeared"))
    }

    /// Create a new user and add them to the default tenant.
    async fn create_new_user(&self, email: &str, default_tenant_id: &str) -> Result<User> {
        let user_id = self
            .db
            .create_user(email)
            .await
            .context("Failed to create user")?;

        self.db
            .set_user_email_verified(user_id)
            .await
            .context("Failed to verify email")?;

        self.db
            .add_org_membership(default_tenant_id, user_id, UserRole::Owner, None)
            .await
            .context("Failed to add org membership")?;

        println!(
            "[AUTH] New user created: user_id={}, email={}, tenant={}, role=Owner",
            user_id,
            anonymize_email(email),
            default_tenant_id
        );

        self.db
            .get_user_by_id(user_id)
            .await
            .context("Failed to get new user")?
            .ok_or_else(|| anyhow!("User creation failed"))
    }

    /// Create a session for a user and return the session token.
    async fn create_user_session(
        &self,
        user_id: uuid::Uuid,
        tenant_id: &str,
        user_agent: Option<&str>,
        ip_address: Option<&str>,
    ) -> Result<String> {
        let (session_token, session_hash) = Self::generate_token();
        let expires_at = chrono::Utc::now().timestamp_millis()
            + (self.config.session_max_age_days as i64 * 24 * 60 * 60 * 1000);

        self.db
            .create_session(&CreateSessionParams {
                session_id_hash: &session_hash,
                user_id,
                tenant_id,
                expires_at,
                user_agent,
                ip_address,
            })
            .await
            .context("Failed to create session")?;

        Ok(session_token)
    }

    // ========== Session Management ==========

    /// Validate a session and return the authenticated user context.
    pub async fn validate_session(&self, session_token: &str) -> Result<Option<AuthUser>> {
        let session_hash = Self::hash_token(session_token);

        let session = match self
            .db
            .get_session(&session_hash)
            .await
            .context("Failed to get session")?
        {
            Some(s) => s,
            None => return Ok(None),
        };

        if session.is_expired() {
            // Clean up expired session
            self.db
                .delete_session(&session_hash)
                .await
                .context("Failed to delete expired session")?;
            return Ok(None);
        }

        // Get user
        let user = self
            .db
            .get_user_by_id(session.user_id)
            .await
            .context("Failed to get user")?
            .ok_or_else(|| anyhow!("User not found"))?;

        // Get user's role in the session's tenant
        let membership = self
            .db
            .get_org_membership(&session.tenant_id, session.user_id)
            .await
            .context("Failed to get org membership")?
            .ok_or_else(|| anyhow!("User not a member of tenant"))?;

        // Update last active time (fire and forget)
        let _ = self.db.touch_session(&session_hash).await;

        Ok(Some(AuthUser {
            user_id: user.user_id,
            email: user.email,
            tenant_id: session.tenant_id,
            role: membership.role,
        }))
    }

    /// Sign out - invalidate session.
    pub async fn sign_out(&self, session_token: &str) -> Result<()> {
        let session_hash = Self::hash_token(session_token);
        self.db
            .delete_session(&session_hash)
            .await
            .context("Failed to delete session")?;
        Ok(())
    }

    /// Sign out all sessions for a user.
    pub async fn sign_out_all(&self, user_id: uuid::Uuid) -> Result<()> {
        self.db
            .delete_user_sessions(user_id)
            .await
            .context("Failed to delete all sessions")?;
        Ok(())
    }

    // ========== Organization Invitations ==========

    /// Invite a user to an organization.
    pub async fn invite_user(&self, invite: &InviteRequest<'_>) -> Result<()> {
        if !invite.inviter.can_invite() {
            return Err(anyhow!("You don't have permission to invite users"));
        }

        let (raw_token, token_hash) = Self::generate_token();

        let expires_at = chrono::Utc::now().timestamp_millis()
            + (self.config.invitation_expiry_days as i64 * 24 * 60 * 60 * 1000);

        self.db
            .create_invitation(&CreateInvitationParams {
                token_hash: &token_hash,
                tenant_id: invite.tenant_id,
                email: invite.email,
                role: invite.role,
                invited_by: invite.inviter.user_id,
                expires_at,
            })
            .await
            .context("Failed to create invitation")?;

        println!(
            "[AUTH] User invitation created: inviter_user_id={}, inviter_email={}, invitee_email={}, tenant={}, role={:?}",
            invite.inviter.user_id,
            anonymize_email(&invite.inviter.email),
            anonymize_email(invite.email),
            invite.tenant_id,
            invite.role
        );

        self.email
            .send_invitation(
                invite.email,
                invite.tenant_name,
                &invite.inviter.email,
                &raw_token,
            )
            .await
            .context("Failed to send invitation email")?;

        Ok(())
    }

    /// Accept an organization invitation.
    pub async fn accept_invitation(
        &self,
        token: &str,
        user_agent: Option<&str>,
        ip_address: Option<&str>,
    ) -> Result<(String, User, String)> {
        let token_hash = Self::hash_token(token);

        // Get and validate invitation
        let invitation = self
            .db
            .get_invitation(&token_hash)
            .await
            .context("Failed to get invitation")?
            .ok_or_else(|| anyhow!("Invalid or expired invitation"))?;

        if !invitation.is_valid() {
            eprintln!(
                "[AUTH] Invitation acceptance failed for {}: invitation expired or already used",
                anonymize_email(&invitation.email)
            );
            return Err(anyhow!("Invitation is expired or already accepted"));
        }

        // Mark invitation as accepted
        self.db
            .mark_invitation_accepted(&token_hash)
            .await
            .context("Failed to mark invitation as accepted")?;

        // Get or create user
        let user = self.get_or_create_invited_user(&invitation.email).await?;

        // Add user to organization
        self.db
            .add_org_membership(
                &invitation.tenant_id,
                user.user_id,
                invitation.role,
                Some(invitation.invited_by),
            )
            .await
            .context("Failed to add org membership")?;

        println!(
            "[AUTH] User accepted invitation: user_id={}, email={}, tenant={}, role={:?}, invited_by={}{}",
            user.user_id,
            anonymize_email(&user.email),
            invitation.tenant_id,
            invitation.role,
            invitation.invited_by,
            ip_address
                .map(|ip| format!(", ip={}", ip))
                .unwrap_or_default()
        );

        // Create session
        let session_token = self
            .create_user_session(user.user_id, &invitation.tenant_id, user_agent, ip_address)
            .await?;

        Ok((session_token, user, invitation.tenant_id))
    }

    /// Get or create a user for invitation flow (doesn't add to tenant).
    async fn get_or_create_invited_user(&self, email: &str) -> Result<User> {
        match self
            .db
            .get_user_by_email(email)
            .await
            .context("Failed to get user")?
        {
            Some(user) => {
                println!(
                    "[AUTH] Existing user accepting invitation: user_id={}, email={}",
                    user.user_id,
                    anonymize_email(email)
                );
                Ok(user)
            }
            None => {
                let user_id = self
                    .db
                    .create_user(email)
                    .await
                    .context("Failed to create user")?;

                self.db
                    .set_user_email_verified(user_id)
                    .await
                    .context("Failed to verify email")?;

                println!(
                    "[AUTH] New user created via invitation: user_id={}, email={}",
                    user_id,
                    anonymize_email(email)
                );

                self.db
                    .get_user_by_id(user_id)
                    .await
                    .context("Failed to get new user")?
                    .ok_or_else(|| anyhow!("User creation failed"))
            }
        }
    }

    // ========== User Info ==========

    /// Get user's organizations.
    pub async fn get_user_organizations(&self, user_id: uuid::Uuid) -> Result<Vec<OrgMembership>> {
        self.db
            .get_user_orgs(user_id)
            .await
            .context("Failed to get user organizations")
    }

    /// Get user by ID.
    pub async fn get_user_by_id(&self, user_id: uuid::Uuid) -> Result<Option<User>> {
        self.db
            .get_user_by_id(user_id)
            .await
            .context("Failed to get user")
    }

    /// Get multiple users by IDs in a single query.
    pub async fn get_users_by_ids(&self, user_ids: Vec<uuid::Uuid>) -> Result<Vec<User>> {
        self.db
            .get_users_by_ids(user_ids)
            .await
            .context("Failed to get users")
    }

    /// Get organization members.
    pub async fn get_org_members(
        &self,
        tenant_id: &str,
        requester: &AuthUser,
    ) -> Result<Vec<OrgMembership>> {
        if !requester.can_manage_settings() {
            return Err(anyhow!("You don't have permission to view members"));
        }

        self.db
            .get_org_members(tenant_id)
            .await
            .context("Failed to get org members")
    }

    /// Remove a member from an organization.
    pub async fn remove_member(
        &self,
        tenant_id: &str,
        user_id: uuid::Uuid,
        requester: &AuthUser,
    ) -> Result<()> {
        if !requester.can_remove_members() {
            return Err(anyhow!("You don't have permission to remove members"));
        }

        // Can't remove yourself
        if requester.user_id == user_id {
            return Err(anyhow!("You can't remove yourself"));
        }

        // Get target's role
        let target_membership = self
            .db
            .get_org_membership(tenant_id, user_id)
            .await
            .context("Failed to get target membership")?
            .ok_or_else(|| anyhow!("User is not a member"))?;

        // Only owner can remove admins
        if target_membership.role == UserRole::Admin && requester.role != UserRole::Owner {
            return Err(anyhow!("Only owners can remove admins"));
        }

        // Can't remove owners
        if target_membership.role == UserRole::Owner {
            return Err(anyhow!("Owners can't be removed"));
        }

        self.db
            .remove_org_membership(tenant_id, user_id)
            .await
            .context("Failed to remove member")?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_token_produces_unique_tokens() {
        let (token1, _) = AuthService::generate_token();
        let (token2, _) = AuthService::generate_token();
        assert_ne!(token1, token2);
    }

    #[test]
    fn test_generate_token_produces_valid_hex() {
        let (token, hash) = AuthService::generate_token();
        assert_eq!(token.len(), 64); // 32 bytes = 64 hex chars
        assert_eq!(hash.len(), 64); // SHA-256 = 64 hex chars
        assert!(hex::decode(&token).is_ok());
        assert!(hex::decode(&hash).is_ok());
    }

    #[test]
    fn test_hash_token_is_deterministic() {
        let token = "test_token_123";
        let hash1 = AuthService::hash_token(token);
        let hash2 = AuthService::hash_token(token);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_hash_token_differs_for_different_tokens() {
        let hash1 = AuthService::hash_token("token1");
        let hash2 = AuthService::hash_token("token2");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_auth_config_defaults() {
        // Clear env vars to test defaults
        env::remove_var("SESSION_MAX_AGE_DAYS");
        env::remove_var("MAGIC_LINK_EXPIRY_MINUTES");
        env::remove_var("INVITATION_EXPIRY_DAYS");

        let config = AuthConfig::from_env();
        assert_eq!(config.session_max_age_days, 365);
        assert_eq!(config.magic_link_expiry_minutes, 15);
        assert_eq!(config.invitation_expiry_days, 7);
    }
}
