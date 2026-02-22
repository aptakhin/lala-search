-- SPDX-License-Identifier: BSD-3-Clause
-- Copyright (c) 2026 Aleksandr Ptakhin
--
-- LalaSearch PostgreSQL Schema
-- Design rationale: docs/cassandra-to-postgres-analysis.md
--
-- Column ordering follows PostgreSQL alignment rules:
--   1. Fixed 8-byte: UUID, TIMESTAMPTZ, BIGINT
--   2. Fixed 4-byte: INTEGER
--   3. Fixed 2-byte: SMALLINT, BOOLEAN
--   4. Variable-length: TEXT
--
-- Multi-tenancy: Row-Level Security (RLS) on tenant tables.
-- Tenant isolation enforced via: SET LOCAL app.current_tenant = '<uuid>';
--
-- Soft deletion: deleted_at TIMESTAMPTZ on tenants, users, allowed_domains,
-- org_memberships. NULL = active, non-NULL = soft-deleted.

-- ============================================================================
-- System Tables (shared across all tenants, no RLS)
-- ============================================================================

-- Global tenant registry
-- Single-tenant mode: one row (created at startup)
-- Multi-tenant mode: one row per customer
CREATE TABLE IF NOT EXISTS tenants (
    -- 8-byte aligned
    tenant_id   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at  TIMESTAMPTZ,
    -- Variable-length
    name        TEXT NOT NULL
);

-- Global user registry
CREATE TABLE IF NOT EXISTS users (
    -- 8-byte aligned
    user_id        UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_login_at  TIMESTAMPTZ,
    deleted_at     TIMESTAMPTZ,
    -- 2-byte aligned
    email_verified BOOLEAN NOT NULL DEFAULT FALSE,
    -- Variable-length
    email          TEXT NOT NULL,
    status         TEXT NOT NULL DEFAULT 'active'  -- active, suspended, deleted
);

-- Unique email only among active users (soft-deleted emails can be reused)
CREATE UNIQUE INDEX IF NOT EXISTS idx_users_email_active
    ON users (email) WHERE deleted_at IS NULL;

-- Server-side sessions (ephemeral, hard-deleted on expiry)
CREATE TABLE IF NOT EXISTS sessions (
    -- 8-byte aligned
    user_id        UUID NOT NULL REFERENCES users(user_id),
    tenant_id      UUID NOT NULL REFERENCES tenants(tenant_id),
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at     TIMESTAMPTZ NOT NULL,
    last_active_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Variable-length
    session_id     TEXT PRIMARY KEY,  -- SHA-256 hash of actual token
    user_agent     TEXT,
    ip_address     TEXT
);

CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions (user_id);

-- Magic link tokens for passwordless auth (ephemeral, hard-deleted on expiry)
CREATE TABLE IF NOT EXISTS magic_link_tokens (
    -- 8-byte aligned
    tenant_id    UUID REFERENCES tenants(tenant_id),  -- nullable: null for new signups
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at   TIMESTAMPTZ NOT NULL,
    -- 2-byte aligned
    used         BOOLEAN NOT NULL DEFAULT FALSE,
    -- Variable-length
    token_hash   TEXT PRIMARY KEY,  -- SHA-256 hash of actual token
    email        TEXT NOT NULL,
    redirect_url TEXT
);

-- Organization memberships (user-to-tenant membership with roles)
CREATE TABLE IF NOT EXISTS org_memberships (
    -- 8-byte aligned
    tenant_id   UUID NOT NULL REFERENCES tenants(tenant_id),
    user_id     UUID NOT NULL REFERENCES users(user_id),
    invited_by  UUID REFERENCES users(user_id),
    joined_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at  TIMESTAMPTZ,
    -- Variable-length
    role        TEXT NOT NULL DEFAULT 'member',  -- owner, admin, member
    PRIMARY KEY (tenant_id, user_id)
);

-- Reverse lookup: find all orgs for a user
CREATE INDEX IF NOT EXISTS idx_org_memberships_user ON org_memberships (user_id);

-- Organization invitations (ephemeral, hard-deleted on expiry)
CREATE TABLE IF NOT EXISTS org_invitations (
    -- 8-byte aligned
    tenant_id   UUID NOT NULL REFERENCES tenants(tenant_id),
    invited_by  UUID NOT NULL REFERENCES users(user_id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at  TIMESTAMPTZ NOT NULL,
    -- 2-byte aligned
    accepted    BOOLEAN NOT NULL DEFAULT FALSE,
    -- Variable-length
    token_hash  TEXT PRIMARY KEY,  -- SHA-256 hash of actual token
    email       TEXT NOT NULL,
    role        TEXT NOT NULL DEFAULT 'member'
);

-- ============================================================================
-- Tenant Tables (RLS-protected via tenant_id)
-- ============================================================================

-- Crawled pages metadata (hard-deleted — content lives in S3, can be re-crawled)
CREATE TABLE IF NOT EXISTS crawled_pages (
    -- 8-byte aligned
    page_id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id             UUID NOT NULL REFERENCES tenants(tenant_id),
    storage_id            UUID,
    last_crawled_at       TIMESTAMPTZ,
    next_crawl_at         TIMESTAMPTZ,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- 4-byte aligned
    crawl_frequency_hours INTEGER NOT NULL DEFAULT 24,
    http_status           INTEGER,
    content_length        INTEGER,
    crawl_count           INTEGER NOT NULL DEFAULT 0,
    -- 2-byte aligned
    storage_compression   SMALLINT NOT NULL DEFAULT 0,  -- 0=none, 1=gzip
    robots_allowed        BOOLEAN NOT NULL DEFAULT TRUE,
    -- Variable-length
    domain                TEXT NOT NULL,
    url_path              TEXT NOT NULL,
    url                   TEXT NOT NULL,
    content_hash          TEXT,
    error_message         TEXT,
    UNIQUE (tenant_id, domain, url_path)
);

-- Crawl queue — job queue for URLs to crawl/re-crawl
-- Use SELECT ... FOR UPDATE SKIP LOCKED for concurrent workers
CREATE TABLE IF NOT EXISTS crawl_queue (
    -- 8-byte aligned
    queue_id        UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id       UUID NOT NULL REFERENCES tenants(tenant_id),
    last_attempt_at TIMESTAMPTZ,
    scheduled_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- 4-byte aligned
    priority        INTEGER NOT NULL DEFAULT 0,
    attempt_count   INTEGER NOT NULL DEFAULT 0,
    -- Variable-length
    url             TEXT NOT NULL,
    domain          TEXT NOT NULL,
    UNIQUE (tenant_id, url)
);

-- Allowed domains whitelist (soft-deleted — may want to re-allow)
CREATE TABLE IF NOT EXISTS allowed_domains (
    -- 8-byte aligned
    tenant_id  UUID NOT NULL REFERENCES tenants(tenant_id),
    added_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,
    -- Variable-length
    domain     TEXT NOT NULL,
    added_by   TEXT,
    notes      TEXT,
    PRIMARY KEY (tenant_id, domain)
);

-- Unique domain only among active entries (soft-deleted domains can be re-added)
CREATE UNIQUE INDEX IF NOT EXISTS idx_allowed_domains_active
    ON allowed_domains (tenant_id, domain) WHERE deleted_at IS NULL;

-- Robots.txt cache (ephemeral cache, can be evicted and re-fetched)
CREATE TABLE IF NOT EXISTS robots_cache (
    -- 8-byte aligned
    tenant_id   UUID NOT NULL REFERENCES tenants(tenant_id),
    fetched_at  TIMESTAMPTZ,
    expires_at  TIMESTAMPTZ,
    -- Variable-length
    domain      TEXT NOT NULL,
    robots_txt  TEXT,
    fetch_error TEXT,
    PRIMARY KEY (tenant_id, domain)
);

-- Crawl errors log (append-only, may be purged by age)
CREATE TABLE IF NOT EXISTS crawl_errors (
    -- 8-byte aligned
    error_id      UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id     UUID NOT NULL REFERENCES tenants(tenant_id),
    page_id       UUID REFERENCES crawled_pages(page_id),
    occurred_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- 4-byte aligned
    attempt_count INTEGER NOT NULL DEFAULT 0,
    -- Variable-length
    domain        TEXT NOT NULL,
    url           TEXT NOT NULL,
    error_type    TEXT NOT NULL,
    error_message TEXT,
    stack_trace   TEXT
);

-- Per-tenant settings (key-value, overwritten not deleted)
CREATE TABLE IF NOT EXISTS settings (
    -- 8-byte aligned
    tenant_id     UUID NOT NULL REFERENCES tenants(tenant_id),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Variable-length
    setting_key   TEXT NOT NULL,
    setting_value TEXT,
    PRIMARY KEY (tenant_id, setting_key)
);

-- ============================================================================
-- Indexes
-- ============================================================================

-- crawled_pages: lookup by tenant + domain + path (covered by UNIQUE constraint)
-- crawled_pages: recrawl scheduling (partial — only pages with next_crawl_at set)
CREATE INDEX IF NOT EXISTS idx_crawled_pages_next_crawl
    ON crawled_pages (tenant_id, next_crawl_at)
    WHERE next_crawl_at IS NOT NULL;

-- crawl_queue: job queue polling order
CREATE INDEX IF NOT EXISTS idx_crawl_queue_poll
    ON crawl_queue (tenant_id, priority, scheduled_at);

-- crawl_errors: recent errors lookup
CREATE INDEX IF NOT EXISTS idx_crawl_errors_tenant_time
    ON crawl_errors (tenant_id, occurred_at DESC);

-- ============================================================================
-- Row-Level Security (RLS) for tenant tables
-- Tenant isolation enforced at DB level.
-- Application sets: SET LOCAL app.current_tenant = '<tenant-uuid>';
-- ============================================================================

ALTER TABLE crawled_pages ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON crawled_pages
    FOR ALL USING (tenant_id = current_setting('app.current_tenant')::uuid)
    WITH CHECK (tenant_id = current_setting('app.current_tenant')::uuid);

ALTER TABLE crawl_queue ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON crawl_queue
    FOR ALL USING (tenant_id = current_setting('app.current_tenant')::uuid)
    WITH CHECK (tenant_id = current_setting('app.current_tenant')::uuid);

ALTER TABLE allowed_domains ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON allowed_domains
    FOR ALL USING (tenant_id = current_setting('app.current_tenant')::uuid)
    WITH CHECK (tenant_id = current_setting('app.current_tenant')::uuid);

ALTER TABLE robots_cache ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON robots_cache
    FOR ALL USING (tenant_id = current_setting('app.current_tenant')::uuid)
    WITH CHECK (tenant_id = current_setting('app.current_tenant')::uuid);

ALTER TABLE crawl_errors ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON crawl_errors
    FOR ALL USING (tenant_id = current_setting('app.current_tenant')::uuid)
    WITH CHECK (tenant_id = current_setting('app.current_tenant')::uuid);

ALTER TABLE settings ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON settings
    FOR ALL USING (tenant_id = current_setting('app.current_tenant')::uuid)
    WITH CHECK (tenant_id = current_setting('app.current_tenant')::uuid);
