# Multi-tenancy Architecture

## Overview

LalaSearch supports two deployment modes, controlled by the `DEPLOYMENT_MODE` environment variable:

| Mode | Value | Use Case |
|------|-------|----------|
| Single-tenant | `single_tenant` | Community edition — self-hosted, fully open source |
| Multi-tenant | `multi_tenant` | SaaS hosted version — one keyspace per customer |

The codebase is shared between both modes. Multi-tenancy infrastructure is currently
**open source** in this repository. This may change as the SaaS offering matures — billing,
tenant provisioning, and payment processing code may be extracted to a private repository.
The Community (single-tenant) edition will always remain fully open source.

## Keyspace Layout

```
lalasearch_system          ← global registry (tenants table, future billing)
lalasearch_<tenant_id>     ← per-tenant data (crawling tables)
```

| Keyspace | Env var | Default | Contains |
|----------|---------|---------|----------|
| System | `CASSANDRA_SYSTEM_KEYSPACE` | `lalasearch_system` | `tenants` table |
| Tenant | `CASSANDRA_KEYSPACE` | `lalasearch_default` | All crawling tables |

**Single-tenant mode**: system keyspace has one `tenant_id = 'default'` row; data keyspace is
`lalasearch_default` (or whatever `CASSANDRA_KEYSPACE` is set to).

**Multi-tenant mode**: each customer gets their own data keyspace, e.g. `lalasearch_acme`. The
system keyspace has one row per customer.

## Design: One Keyspace Per Tenant

All Cassandra queries use fully qualified table names (`keyspace.table`) instead of relying on a
`USE keyspace` session state. This means:

1. The `CassandraClient` stores the target keyspace name
2. All queries embed the keyspace: `SELECT * FROM lalasearch_acme.crawled_pages WHERE ...`
3. In multi-tenant mode, scope a request: `db_client.with_keyspace("lalasearch_acme")`
4. **One shared connection pool** serves all tenants — no switching, no per-tenant connections

### Why Not `tenant_id` Column?

Adding a `tenant_id` column to every row was considered and rejected:

- Wastes storage on every row in single-tenant mode
- Requires schema changes to all tables
- Every query needs `WHERE tenant_id = ?` — easy to forget
- Keyspace isolation is stronger and Cassandra-native

### Why Not `USE keyspace` Per Request?

`session.use_keyspace()` in the Scylla/Cassandra driver is **session-level**, not request-level.
In an async server with a shared `Arc<Session>`, calling it per-request would race with concurrent
requests from other tenants. Fully qualified table names solve this with zero overhead.

## Cassandra Keyspace Naming Rules

Tenant IDs used in keyspace names must follow Cassandra naming constraints:

- Only letters, digits, and underscores: `[a-z0-9_]`
- Must start with a letter or underscore
- Maximum 37 characters (48 max minus the 11-char `lalasearch_` prefix)
- Case-insensitive (Cassandra stores lowercase)

Valid: `lalasearch_acme`, `lalasearch_tech_corp`
Invalid: `lalasearch_acme-corp` (hyphen), `lalasearch_1st` (starts with digit)

## Code Hook Points

In single-tenant mode, handlers use the default client directly:

```rust
// Single-tenant: AppState.db_client points to lalasearch_default
state.db_client.is_domain_allowed(&domain).await
```

In multi-tenant mode, scope the client to the authenticated tenant's keyspace:

```rust
// Multi-tenant: scope to tenant keyspace extracted from auth header
let tenant_keyspace = format!("lalasearch_{}", tenant_id);
let tenant_db = state.db_client.with_keyspace(&tenant_keyspace);
tenant_db.is_domain_allowed(&domain).await
```

The core crawling, storage, search, and queue logic is **identical** between both modes.

## What the SaaS Version Adds

The open source codebase is designed so adding SaaS functionality requires minimal code:

1. **Auth middleware**: Extract `tenant_id` from JWT or API key in request headers
2. **Tenant routing**: Call `db_client.with_keyspace(tenant_keyspace)` per request (one line change)
3. **Tenant provisioning**: API endpoint to create a keyspace and apply `schema.cql`
4. **Billing** (future, currently open source): Payment processing and usage metering

Note: SaaS-specific code (particularly billing and tenant provisioning) is currently open
source in this repository but may move to a private repository as the hosted offering matures.
The Community (single-tenant) edition will always remain fully open source.

## Environment Variables

| Variable | Single-tenant default | Multi-tenant |
|----------|----------------------|--------------|
| `DEPLOYMENT_MODE` | `single_tenant` | `multi_tenant` |
| `CASSANDRA_KEYSPACE` | `lalasearch_default` | Tenant data keyspace |
| `CASSANDRA_SYSTEM_KEYSPACE` | `lalasearch_system` | `lalasearch_system` |
