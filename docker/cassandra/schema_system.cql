-- SPDX-License-Identifier: BSD-3-Clause
-- Copyright (c) 2026 Aleksandr Ptakhin

-- System keyspace for LalaSearch - stores global tenant registry
-- This keyspace is shared across all deployments.
-- Note: SYSTEM_KEYSPACE_NAME is replaced at runtime
CREATE KEYSPACE IF NOT EXISTS ${SYSTEM_KEYSPACE_NAME}
WITH replication = {
    'class': 'SimpleStrategy',
    'replication_factor': 1
};

USE ${SYSTEM_KEYSPACE_NAME};

-- Global tenant registry
-- Single-tenant mode: always one row with tenant_id = 'default'
-- Multi-tenant mode: one row per customer, inserted during tenant provisioning
CREATE TABLE IF NOT EXISTS tenants (
    tenant_id text PRIMARY KEY,
    name text,
    created_at timestamp
) WITH comment = 'Global tenant registry. Single-tenant: one default row. Multi-tenant: one row per customer.';

-- ============================================================================
-- Authentication Tables
-- ============================================================================

-- Users table - global user registry
CREATE TABLE IF NOT EXISTS users (
    user_id uuid PRIMARY KEY,
    email text,
    email_verified boolean,
    created_at timestamp,
    updated_at timestamp,
    last_login_at timestamp,
    status text  -- active, suspended, deleted
) WITH comment = 'Global user registry.';

-- Secondary index for email lookup (login flow)
CREATE INDEX IF NOT EXISTS idx_users_email ON users (email);

-- Sessions - long-lived server-side sessions
CREATE TABLE IF NOT EXISTS sessions (
    session_id text PRIMARY KEY,  -- SHA-256 hash of actual session token
    user_id uuid,
    tenant_id text,               -- Currently active tenant for this session
    created_at timestamp,
    expires_at timestamp,
    last_active_at timestamp,
    user_agent text,
    ip_address text
) WITH comment = 'Server-side sessions. Expiry configured via SESSION_MAX_AGE_DAYS env var.';

-- Index for looking up all sessions by user (for logout-all)
CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions (user_id);

-- Magic link tokens - short-lived for passwordless auth
CREATE TABLE IF NOT EXISTS magic_link_tokens (
    token text PRIMARY KEY,       -- SHA-256 hash of actual token
    email text,
    tenant_id text,               -- null for new signups, set for org invitations
    redirect_url text,            -- Where to redirect after verification
    created_at timestamp,
    expires_at timestamp,
    used boolean
) WITH comment = 'Magic link tokens for passwordless authentication. Expiry configured via MAGIC_LINK_EXPIRY_MINUTES env var.';

-- Organization memberships - maps users to tenants with roles
CREATE TABLE IF NOT EXISTS org_memberships (
    tenant_id text,
    user_id uuid,
    role text,                    -- owner, admin, member
    joined_at timestamp,
    invited_by uuid,
    PRIMARY KEY (tenant_id, user_id)
) WITH comment = 'Maps users to organizations with roles.';

-- Reverse lookup: find all orgs for a user
CREATE TABLE IF NOT EXISTS user_orgs (
    user_id uuid,
    tenant_id text,
    role text,
    joined_at timestamp,
    PRIMARY KEY (user_id, tenant_id)
) WITH comment = 'Reverse lookup: all organizations a user belongs to.';

-- Organization invitations
CREATE TABLE IF NOT EXISTS org_invitations (
    token text PRIMARY KEY,       -- SHA-256 hash of actual token
    tenant_id text,
    email text,
    role text,                    -- Role to assign when accepted
    invited_by uuid,
    created_at timestamp,
    expires_at timestamp,
    accepted boolean
) WITH comment = 'Pending organization invitations. Expiry configured via INVITATION_EXPIRY_DAYS env var.';
