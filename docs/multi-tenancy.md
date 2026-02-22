# Multi-tenancy Architecture

## Overview

LalaSearch supports two deployment modes, controlled by the `DEPLOYMENT_MODE` environment variable:

| Mode | Value | Use Case |
|------|-------|----------|
| Single-tenant | `single_tenant` | Community edition — self-hosted, fully open source |
| Multi-tenant | `multi_tenant` | SaaS hosted version — tenant isolation via PostgreSQL RLS |

The codebase is shared between both modes. Multi-tenancy infrastructure is currently
**open source** in this repository. This may change as the SaaS offering matures — billing,
tenant provisioning, and payment processing code may be extracted to a private repository.
The Community (single-tenant) edition will always remain fully open source.

## Design: Row-Level Security (RLS)

Multi-tenancy uses PostgreSQL Row-Level Security to isolate tenant data within a single database:

1. All tenant-scoped tables have a `tenant_id UUID NOT NULL` column
2. RLS policies restrict rows to the tenant set via `SET LOCAL app.current_tenant`
3. The `DbClient` sets the session variable at the start of each transaction
4. **One shared connection pool** serves all tenants — no per-tenant databases or schemas

### How It Works

```sql
-- RLS policy on each tenant table (e.g., crawled_pages)
CREATE POLICY tenant_isolation ON crawled_pages
  USING (tenant_id = current_setting('app.current_tenant')::uuid);

ALTER TABLE crawled_pages ENABLE ROW LEVEL SECURITY;
```

```rust
// In Rust, the DbClient stores tenant_id and sets the session variable
let db = state.db_client.with_tenant(tenant_id);
// All subsequent queries through this client are automatically scoped
db.is_domain_allowed(&domain).await
```

### Why RLS Over Separate Schemas/Databases?

- **Simpler operations**: Single database, single schema, standard migrations
- **Better resource utilization**: Shared connection pool, no per-tenant overhead
- **Standard PostgreSQL feature**: Well-tested, no custom code needed
- **JOIN-friendly**: Cross-tenant admin queries possible when RLS is bypassed
- **Easy to reason about**: All data in one place with policy-enforced isolation

## Tenant Layout

```
PostgreSQL (lalasearch)
  ├── tenants               ← global registry (not RLS-scoped)
  ├── users                 ← global user accounts (not RLS-scoped)
  ├── sessions              ← global sessions (not RLS-scoped)
  ├── org_memberships       ← user-tenant mapping (not RLS-scoped)
  ├── crawl_queue           ← per-tenant (RLS-scoped)
  ├── crawled_pages         ← per-tenant (RLS-scoped)
  ├── crawl_errors          ← per-tenant (RLS-scoped)
  ├── allowed_domains       ← per-tenant (RLS-scoped)
  ├── robots_cache          ← per-tenant (RLS-scoped)
  └── settings              ← per-tenant (RLS-scoped)
```

**Single-tenant mode**: A default tenant is created with `DEFAULT_TENANT_ID` (env var, defaults
to `00000000-0000-0000-0000-000000000001`). All data uses this single tenant ID.

**Multi-tenant mode**: Each organization gets its own tenant_id (UUID). The session cookie
determines which tenant's data is accessible.

## Code Hook Points

In single-tenant mode, handlers use the default client directly:

```rust
// Single-tenant: AppState.db_client points to the default tenant
state.db_client.is_domain_allowed(&domain).await
```

In multi-tenant mode, the `TenantDb` extractor resolves the tenant from the session:

```rust
// Multi-tenant: TenantDb extractor validates session and scopes to tenant
async fn add_domain_handler(
    TenantDb(db): TenantDb,  // automatically scoped to authenticated tenant
    Json(payload): Json<AddDomainRequest>,
) -> Result<Json<AddDomainResponse>, (StatusCode, String)> {
    db.insert_allowed_domain(&payload.domain, "api", payload.notes.as_deref()).await
}
```

The core crawling, storage, search, and queue logic is **identical** between both modes.

## What the SaaS Version Adds

The open source codebase is designed so adding SaaS functionality requires minimal code:

1. **Auth middleware**: Extract `tenant_id` from session cookie
2. **Tenant routing**: `TenantDb` extractor calls `db_client.with_tenant(tenant_id)` per request
3. **Tenant provisioning**: Create a row in `tenants` table (no schema creation needed)
4. **Billing** (future, currently open source): Payment processing and usage metering

Note: SaaS-specific code (particularly billing and tenant provisioning) is currently open
source in this repository but may move to a private repository as the hosted offering matures.
The Community (single-tenant) edition will always remain fully open source.

## Environment Variables

| Variable | Single-tenant default | Multi-tenant |
|----------|----------------------|--------------|
| `DEPLOYMENT_MODE` | `single_tenant` | `multi_tenant` |
| `DATABASE_URL` | `postgres://lalasearch:lalasearch@postgres:5432/lalasearch` | Same (single shared database) |
| `DEFAULT_TENANT_ID` | `00000000-0000-0000-0000-000000000001` | Same (used for default tenant) |
