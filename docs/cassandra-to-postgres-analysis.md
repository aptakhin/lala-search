# Cassandra → PostgreSQL Migration Analysis

## Why This Makes Sense for LalaSearch

Current data volumes are **modest** — ~1,000 pages/hour, ~50K domain lookups/hour. Cassandra's strengths (massive horizontal scale, multi-DC replication) are overkill here. The workload is actually a **poor fit** for Cassandra in several ways:

1. **Dual writes for bidirectional lookups** (`org_memberships` + `user_orgs`) — PostgreSQL handles this with a single table + index
2. **`ALLOW FILTERING` on secondary indexes** (user email lookup, session lookups) — this is an anti-pattern in Cassandra
3. **No JOINs** forces denormalization and N+1 query risks during link extraction
4. **Counter tables** (`crawl_stats`) have known Cassandra quirks (no TTL, no deletes, eventual consistency) — PostgreSQL can aggregate directly from raw data, eliminating this table entirely
5. **Queue pattern** uses optimistic DELETE-based concurrency — PostgreSQL has `SELECT FOR UPDATE SKIP LOCKED` which is strictly better

---

## Pros of PostgreSQL

| Area | Benefit |
|------|---------|
| **Transactions** | Atomic auth flows — no more dual writes to `org_memberships` + `user_orgs` |
| **Querying** | JOINs, subqueries, window functions, no `ALLOW FILTERING` |
| **Queue** | `SELECT FOR UPDATE SKIP LOCKED` is purpose-built for job queues |
| **Multi-tenancy** | Row-Level Security (RLS) — enforced at DB level, not just app level |
| **Data integrity** | Foreign keys, CHECK constraints, unique constraints |
| **Tooling** | pg_dump, pgAdmin, extensive monitoring, every ORM supports it |
| **Operations** | Single database to manage instead of Cassandra cluster |
| **Schema evolution** | `ALTER TABLE` is straightforward, mature migration tools (sqlx, refinery) |

---

## Cons / Risks

| Area | Risk | Mitigation |
|------|------|------------|
| **Write throughput** | Single-writer bottleneck at very high scale | Batched inserts (correct instinct), but at current volumes (~1K pages/hr) this is a non-issue |
| **Queue polling** | High-frequency polling can create contention | `LISTEN/NOTIFY` for instant wake-up instead of polling every 5s |
| **Counter aggregation** | No native counters like Cassandra | `UPDATE SET count = count + 1` works fine; or move stats to ClickHouse later |
| **Multi-tenant isolation** | Weaker than keyspace-per-tenant | RLS policies + `tenant_id` column; or schema-per-tenant if needed |
| **Horizontal scaling** | Can't add nodes like Cassandra | Not needed at current scale; read replicas cover read scaling |

---

## Queue Pattern in PostgreSQL

PostgreSQL comfortably handles **10K+ individual inserts/second** on modest hardware. The current crawl pipeline does maybe 20 inserts/second at peak. Here's the recommended pattern:

```sql
-- Job queue with SKIP LOCKED (replaces Cassandra's delete-based concurrency)
SELECT * FROM crawl_queue
WHERE scheduled_at <= NOW()
ORDER BY priority, scheduled_at
LIMIT 1
FOR UPDATE SKIP LOCKED;

-- Instant notification instead of 5s polling
NOTIFY crawl_queue_ready;
```

This is **strictly superior** to the current Cassandra approach — no optimistic delete races, no lost entries, proper transaction isolation.

---

## Where Batching Actually Helps

For the crawl pipeline, batching matters in **link extraction** — when a page has 200 links, instead of 200 individual `INSERT INTO crawl_queue`, do:

```sql
INSERT INTO crawl_queue (url, domain, priority, scheduled_at)
VALUES ($1, $2, $3, $4), ($5, $6, $7, $8), ...
ON CONFLICT (url) DO NOTHING;
```

Multi-row INSERT + `ON CONFLICT` is both simpler and faster than 200 individual Cassandra inserts. The `ON CONFLICT` also replaces the current "check if exists then insert" pattern, eliminating the N+1 existence checks during link extraction.

---

## Statistics: No Separate Table Needed

The Cassandra `crawl_stats` counter table is **eliminated**. PostgreSQL can aggregate directly from raw data:

```sql
-- Pages crawled per hour per domain (replaces crawl_stats.pages_crawled counter)
SELECT domain, date_trunc('hour', last_crawled_at) AS hour, COUNT(*) AS pages_crawled
FROM crawled_pages
WHERE tenant_id = $1 AND last_crawled_at >= $2
GROUP BY domain, date_trunc('hour', last_crawled_at);

-- Failures per hour per domain (replaces crawl_stats.pages_failed counter)
SELECT domain, date_trunc('hour', occurred_at) AS hour, COUNT(*) AS pages_failed
FROM crawl_errors
WHERE tenant_id = $1 AND occurred_at >= $2
GROUP BY domain, date_trunc('hour', occurred_at);
```

If these become slow at scale, use a **materialized view** or extract to ClickHouse — no need for a counter table maintained in application code.

### ClickHouse (Later, if needed)

- ClickHouse for analytics when sub-second queries over millions of rows are needed
- Push events from PostgreSQL → ClickHouse via CDC (Change Data Capture) or batch ETL
- Same batched insert concern applies — ClickHouse wants bulk inserts (1K+ rows at a time)

**Recommendation**: Start with PostgreSQL aggregation queries. Extract to ClickHouse only when they become a bottleneck.

---

## Multi-Tenancy Recommendation

**Row-level with RLS** (not schema-per-tenant):

```sql
-- All tenant tables get a tenant_id column
ALTER TABLE crawled_pages ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON crawled_pages
  USING (tenant_id = current_setting('app.current_tenant'));
```

This is simpler to manage than schema-per-tenant, gives DB-level enforcement, and works well with connection pooling. The system tables (users, sessions) remain shared — no RLS needed on those, just normal `WHERE tenant_id = ?` in queries.

---

## Migration Complexity

The migration covers 13 Cassandra tables → 11 PostgreSQL tables (2 eliminated):

| Current (Cassandra) | After (PostgreSQL) | Change |
|---------------------|-------------------|--------|
| `org_memberships` + `user_orgs` (dual write) | Single table with indexes | Eliminates dual-write bug risk |
| `crawl_queue` (optimistic DELETE) | `FOR UPDATE SKIP LOCKED` | Proper concurrency, no lost entries |
| `crawl_stats` (Cassandra counters) | *(eliminated)* | Aggregation queries on raw data instead |
| `user_orgs` (reverse lookup) | *(eliminated)* | Index on `org_memberships(user_id)` instead |
| `allowed_domains` check per link (N+1) | `EXISTS` subquery in batch insert | Single query for all links |
| `ALLOW FILTERING` queries | Proper WHERE clauses with indexes | Correct index usage |
| Keyspace-per-tenant isolation | Row-Level Security policies | DB-enforced, simpler management |

---

## Current Tables → PostgreSQL Mapping

### System Tables (shared, no RLS)

| Cassandra Table | PostgreSQL Table | Key Changes |
|-----------------|-----------------|-------------|
| `tenants` | `tenants` | Add proper PK, `created_at` default |
| `users` | `users` | Email gets UNIQUE constraint (no more ALLOW FILTERING) |
| `sessions` | `sessions` | FK to users, index on user_id (no ALLOW FILTERING) |
| `magic_link_tokens` | `magic_link_tokens` | FK to tenants, TTL replaced by scheduled cleanup |
| `org_memberships` | `org_memberships` | Single table replaces both memberships + user_orgs |
| `user_orgs` | *(eliminated)* | Replaced by index on `org_memberships(user_id)` |
| `org_invitations` | `org_invitations` | FK constraints, TTL replaced by scheduled cleanup |

### Tenant Tables (RLS-protected)

| Cassandra Table | PostgreSQL Table | Key Changes |
|-----------------|-----------------|-------------|
| `crawled_pages` | `crawled_pages` | Add `tenant_id`, composite unique on `(tenant_id, domain, url_path)` |
| `crawl_queue` | `crawl_queue` | Add `tenant_id`, use `FOR UPDATE SKIP LOCKED` |
| `allowed_domains` | `allowed_domains` | Add `tenant_id`, unique on `(tenant_id, domain)` |
| `robots_cache` | `robots_cache` | Add `tenant_id`, unique on `(tenant_id, domain)` |
| `crawl_errors` | `crawl_errors` | Add `tenant_id`, time-based partitioning |
| `crawl_stats` | *(eliminated)* | Aggregation queries on `crawled_pages` and `crawl_errors` instead |
| `settings` | `settings` | Add `tenant_id`, unique on `(tenant_id, setting_key)` |

---

## PostgreSQL Schema Design

### Surrogate Keys vs Composite Natural Keys

Some tables currently use composite natural keys (e.g., `crawled_pages` keyed by `(domain, url_path)`). For PostgreSQL, consider using a **surrogate UUID primary key** where:

- The natural key has many text fields (wide composite keys bloat every index that references them)
- Other tables need to reference the row via foreign key (FK to a single UUID is cheaper than FK to 3 text columns)
- The row is referenced frequently in JOINs

| Table | Current Cassandra PK | PostgreSQL Approach | Rationale |
|-------|---------------------|---------------------|-----------|
| `crawled_pages` | `(domain, url_path)` | `page_id UUID` PK + UNIQUE on `(tenant_id, domain, url_path)` | Referenced by crawl_errors, queue; FK on single UUID is cheaper |
| `crawl_queue` | `(priority, scheduled_at, url)` | `queue_id UUID` PK + indexes on `(tenant_id, priority, scheduled_at)` | Composite PK was for Cassandra ordering; PostgreSQL uses indexes instead |
| `crawl_errors` | `(domain, occurred_at, url)` | `error_id UUID` PK + `page_id` FK + index on `(tenant_id, occurred_at)` | Can FK to crawled_pages instead of repeating domain+url |
| `allowed_domains` | `(domain)` | Keep `(tenant_id, domain)` as PK | Simple, small — no benefit from surrogate key |
| `robots_cache` | `(domain)` | Keep `(tenant_id, domain)` as PK | Simple, small — no benefit from surrogate key |
| `settings` | `(setting_key)` | Keep `(tenant_id, setting_key)` as PK | Simple key-value — no benefit from surrogate key |
| `users` | `(user_id)` | Keep `user_id UUID` PK | Already a UUID |
| `sessions` | `(session_id)` | Keep `session_id TEXT` PK | Hash-based, already a single column |

**Rule of thumb**: Use surrogate UUID when the natural key is wide (multiple text columns) and other tables reference it. Keep natural keys when they're simple and self-contained.

### Column Ordering for Space Optimization

PostgreSQL stores rows with alignment padding. Fixed-size types (UUID, BIGINT, TIMESTAMP) are 8-byte aligned. Variable-size types (TEXT, BYTEA) have a 1-4 byte length header. Mixing them carelessly wastes bytes per row to padding.

**Ordering rule**: Place columns in descending alignment order:

```
1. Fixed 8-byte:  UUID, BIGINT, TIMESTAMP, DOUBLE PRECISION
2. Fixed 4-byte:  INTEGER, REAL, DATE
3. Fixed 2-byte:  SMALLINT, BOOLEAN (internally 1 byte but 2-byte aligned in some cases)
4. Variable-size:  TEXT, VARCHAR, BYTEA (no alignment requirement)
```

**Example — `crawled_pages` table:**

```sql
CREATE TABLE crawled_pages (
    -- 8-byte aligned (UUIDs, timestamps first)
    page_id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id            UUID NOT NULL REFERENCES tenants(tenant_id),
    storage_id           UUID,
    last_crawled_at      TIMESTAMPTZ,
    next_crawl_at        TIMESTAMPTZ,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now(),

    -- 4-byte aligned
    crawl_frequency_hours INTEGER NOT NULL DEFAULT 24,
    http_status           INTEGER,
    content_length        INTEGER,
    crawl_count           INTEGER NOT NULL DEFAULT 0,

    -- 2-byte aligned
    storage_compression   SMALLINT NOT NULL DEFAULT 0,
    robots_allowed        BOOLEAN NOT NULL DEFAULT TRUE,

    -- Variable-length (no alignment padding needed)
    domain               TEXT NOT NULL,
    url_path             TEXT NOT NULL,
    url                  TEXT NOT NULL,
    content_hash         TEXT,
    error_message        TEXT,

    UNIQUE (tenant_id, domain, url_path)
);
```

**Savings estimate**: On a table with 100M rows, proper column ordering can save 8-16 bytes/row of padding = **0.8–1.6 GB**. Not dramatic, but free — just a matter of column declaration order.

### Indexes

```sql
-- crawled_pages: primary lookups and recrawl scheduling
CREATE INDEX idx_crawled_pages_tenant_domain ON crawled_pages (tenant_id, domain, url_path);
CREATE INDEX idx_crawled_pages_next_crawl ON crawled_pages (tenant_id, next_crawl_at)
  WHERE next_crawl_at IS NOT NULL;

-- crawl_queue: job queue ordering
CREATE INDEX idx_crawl_queue_poll ON crawl_queue (tenant_id, priority, scheduled_at);

-- crawl_errors: recent errors lookup
CREATE INDEX idx_crawl_errors_tenant_time ON crawl_errors (tenant_id, occurred_at DESC);

-- users: email lookup (replaces ALLOW FILTERING)
CREATE UNIQUE INDEX idx_users_email ON users (email);

-- sessions: logout-all by user (replaces ALLOW FILTERING)
CREATE INDEX idx_sessions_user ON sessions (user_id);

-- org_memberships: both directions covered
CREATE INDEX idx_org_memberships_user ON org_memberships (user_id);
-- (tenant_id, user_id) is already the PK — covers tenant→members lookup
```

Partial indexes (with `WHERE` clause) keep the index small for sparse conditions like `next_crawl_at IS NOT NULL`.

### Soft Deletion Strategy

All entities use soft deletion — no data is ever physically removed via application code.

**Pattern**: Every table with deletable data gets a `deleted_at TIMESTAMPTZ` column:

```sql
-- Soft delete columns (added to relevant tables)
deleted_at TIMESTAMPTZ  -- NULL = active, non-NULL = soft-deleted timestamp
```

**Tables with soft deletion:**

| Table | Why soft delete |
|-------|----------------|
| `tenants` | Preserve tenant history; allow reactivation; child data stays intact |
| `users` | Account recovery; audit trail; preserve org membership history |
| `allowed_domains` | May want to re-allow a domain; crawled data references it |
| `crawled_pages` | Preserve crawl history; storage content still exists in S3 |
| `org_memberships` | Audit trail of who was in which org |

**Tables WITHOUT soft deletion:**

| Table | Why hard delete is fine |
|-------|----------------------|
| `sessions` | Ephemeral by nature; expired sessions can be purged |
| `magic_link_tokens` | Ephemeral; expired tokens can be purged |
| `org_invitations` | Ephemeral; expired/accepted invitations can be purged |
| `crawl_queue` | Transient work items; deleted after processing |
| `crawl_errors` | Append-only log; may be purged by age but never logically deleted |
| `robots_cache` | Cache; can be evicted and re-fetched |
| `settings` | Key-value pairs; overwritten, not deleted |

**Query pattern** — all active-data queries filter out deleted rows:

```sql
-- Active tenants only
SELECT * FROM tenants WHERE deleted_at IS NULL;

-- Active crawled pages for a tenant
SELECT * FROM crawled_pages WHERE tenant_id = $1 AND deleted_at IS NULL;
```

**Partial indexes on active data** — keep indexes lean by excluding deleted rows:

```sql
CREATE UNIQUE INDEX idx_users_email_active ON users (email)
  WHERE deleted_at IS NULL;

CREATE UNIQUE INDEX idx_allowed_domains_active ON allowed_domains (tenant_id, domain)
  WHERE deleted_at IS NULL;

CREATE INDEX idx_crawled_pages_next_crawl ON crawled_pages (tenant_id, next_crawl_at)
  WHERE next_crawl_at IS NOT NULL AND deleted_at IS NULL;
```

This means a soft-deleted user's email can be re-used by a new account, and a soft-deleted domain can be re-added — the uniqueness constraint only applies to active rows.

### Foreign Key Strategy

All FKs use `RESTRICT` (the default) — no cascading deletes. Soft deletion means the parent row stays, so FKs never block child operations.

```sql
-- crawl_errors references crawled_pages — parent is soft-deleted, never removed
ALTER TABLE crawl_errors
  ADD CONSTRAINT fk_crawl_errors_page
  FOREIGN KEY (page_id) REFERENCES crawled_pages(page_id);  -- RESTRICT (default)

-- sessions reference users — user is soft-deleted, session can be hard-deleted independently
ALTER TABLE sessions
  ADD CONSTRAINT fk_sessions_user
  FOREIGN KEY (user_id) REFERENCES users(user_id);  -- RESTRICT (default)

-- org_memberships reference both users and tenants — both soft-deleted, never removed
ALTER TABLE org_memberships
  ADD CONSTRAINT fk_org_memberships_user
  FOREIGN KEY (user_id) REFERENCES users(user_id),  -- RESTRICT
  ADD CONSTRAINT fk_org_memberships_tenant
  FOREIGN KEY (tenant_id) REFERENCES tenants(tenant_id);  -- RESTRICT

-- Tenant tables reference tenants — tenant is soft-deleted, never removed
ALTER TABLE crawled_pages
  ADD CONSTRAINT fk_crawled_pages_tenant
  FOREIGN KEY (tenant_id) REFERENCES tenants(tenant_id);  -- RESTRICT

ALTER TABLE crawl_queue
  ADD CONSTRAINT fk_crawl_queue_tenant
  FOREIGN KEY (tenant_id) REFERENCES tenants(tenant_id);  -- RESTRICT
```

**Why RESTRICT works with soft deletion:**
- Parent rows are never physically deleted → FKs never fire
- `RESTRICT` catches accidental hard deletes (safety net)
- No surprise cascading data loss
- Referential integrity is maintained permanently

**The `crawl_queue` special case**: Queue entries reference URLs not yet crawled, so `page_id` FK is not applicable. The queue has `url` and `domain` as data columns but no FK to `crawled_pages`. After crawling, the queue entry is deleted (hard delete — it's a transient work item).

**Cleanup of ephemeral data**: Sessions, magic link tokens, and invitations are cleaned up by a scheduled job that hard-deletes expired rows. This is safe because no other tables reference them as parents.

```sql
-- Periodic cleanup (run via cron or background task)
DELETE FROM sessions WHERE expires_at < NOW();
DELETE FROM magic_link_tokens WHERE expires_at < NOW();
DELETE FROM org_invitations WHERE expires_at < NOW();
```

---

## Rust Driver / ORM Options

| Crate | Type | Async | Compile-time checks | Notes |
|-------|------|-------|-------------------|-------|
| **sqlx** | Async driver | Yes | Yes (query macros) | Most popular for Axum projects, compile-time SQL verification |
| **diesel** | ORM | Yes (diesel-async) | Yes (schema DSL) | More opinionated, heavier, strong type safety |
| **tokio-postgres** | Low-level driver | Yes | No | Maximum control, minimal abstraction |
| **sea-orm** | ORM | Yes | Partial | ActiveRecord pattern, good for rapid development |

**Recommendation**: **sqlx** — it's the natural fit for an Axum project, provides compile-time SQL checking, and doesn't force an ORM pattern. It matches the current style of writing explicit CQL queries.

---

## Summary

**Go for it.** The workload is relational in nature — the current code fights Cassandra's data model (dual writes, ALLOW FILTERING, no JOINs) more than it benefits from it. PostgreSQL will give simpler code, better data integrity, and easier operations. The queue batching concern is real but straightforward, and the volumes don't require it yet.
