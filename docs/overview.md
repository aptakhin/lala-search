# LalaSearch - Architecture Overview

## Vision

LalaSearch is a self-hosted open source search engine for your private web. Organizations use it to index and search across internal tools — project trackers, communication systems, wikis, knowledge bases, and any authenticated web application — without sending data to third-party services.

The core principle: **your data stays on your infrastructure**.

## Editions

### Community Edition (this repository)

Single-tenant, self-hosted, fully open source under BSD 3-Clause. Deploy on your own server with Docker Compose. Intended for teams that want full control over their search index and data.

### SaaS Edition

Multi-tenant hosted version sharing the same codebase. One Apache Cassandra keyspace per customer. SaaS-specific code (tenant provisioning, billing, payment processing) is currently open source but may move to a separate private repository as the offering matures.

The Community edition will always remain fully open source.

## Architecture

### Core Components

```
┌─────────────────────────────────────────────────────────┐
│                      lala-agent                         │
│                                                         │
│  HTTP API          Crawl Queue       Queue Processor    │
│  (Axum)     ──►   (Cassandra)  ──►  (worker loop)      │
│                                          │              │
│  Search API                         Web Crawler         │
│  (Meilisearch)◄──  Search Index ◄── (robots.txt)       │
│                    (Meilisearch)         │              │
│                                    S3 Storage           │
│                    Metadata ◄──    (SeaweedFS)          │
│                    (Cassandra)                          │
└─────────────────────────────────────────────────────────┘
```

**lala-agent** is the single deployable service. It runs all roles simultaneously in `--mode all` (the default). Future work will allow splitting roles for larger deployments.

### Crawl Pipeline

1. A URL is added to the crawl queue (via API or a connector)
2. The queue processor picks it up, fetches the page, respects `robots.txt`
3. Raw HTML is stored in S3-compatible object storage (SeaweedFS by default)
4. Extracted text is indexed in Meilisearch for full-text search
5. Metadata (URL, timestamps, storage reference) is written to Cassandra

### Connector Model

Integrations with private systems (communication tools, knowledge bases, project trackers) work as **connectors** — components that translate a system's API or web interface into crawl queue entries. A connector handles authentication, pagination, and incremental updates for a specific source.

The crawl pipeline itself is source-agnostic: once a URL is in the queue with the right session/auth context, the rest of the pipeline is identical regardless of where the content came from.

### Authentication

LalaSearch uses passwordless magic-link authentication. Users receive a time-limited email link; clicking it creates an authenticated session cookie.

Organization-based access control (owner / admin / member roles) gates admin operations.

### Web Interface

The `lala-web` service provides a retro 1990s-style frontend served by Nginx:

- **Search** (`/`): Public search interface (multi-tenant) or auto-redirect to sign-in (single-tenant)
- **Sign In** (`/signin`): Magic link email request
- **Dashboard** (`/dashboard`): Invite users, manage allowed domains (authenticated)

## Technology Stack

| Component | Technology | Why |
|-----------|-----------|-----|
| Language | Rust | Performance, memory safety, async concurrency |
| HTTP framework | Axum + Tokio | Ergonomic async web framework |
| Crawl metadata | Apache Cassandra | Horizontally scalable, fast writes, no transactions needed |
| Full-text search | Meilisearch | Fast, open source, simple to operate |
| Object storage | SeaweedFS | Open source S3-compatible, self-hostable |
| Email delivery | SMTP (configurable) | Bring your own mail server or relay |

All dependencies are open source. See [CLAUDE.md](../CLAUDE.md) for the project's open source policy.

## Deployment Modes

Controlled by the `DEPLOYMENT_MODE` environment variable:

- **`single_tenant`** (default): Community edition. One tenant (`default`), one Cassandra keyspace (`lalasearch_default`).
- **`multi_tenant`**: SaaS edition. Auth middleware extracts `tenant_id` from the session; all queries route to the tenant's dedicated keyspace (`lalasearch_<tenant_id>`).

The core crawling, queue, storage, and search logic is identical between modes. See [multi-tenancy.md](multi-tenancy.md) for the keyspace design.

## Data Model

```
Cassandra (lalasearch_system)
  └── tenants              ← global tenant registry

Cassandra (lalasearch_<tenant_id>)
  ├── crawl_queue          ← pending URLs
  ├── crawled_pages        ← metadata + S3 reference
  ├── allowed_domains      ← domain allowlist
  └── settings             ← per-tenant runtime config

SeaweedFS / S3
  └── lalasearch-content/
        ├── <uuid>.html    ← raw HTML (uncompressed)
        └── <uuid>.html.gz ← raw HTML (gzip)

Meilisearch
  └── <tenant_id>          ← full-text index per tenant
```

## Development Principles

1. **Open source first**: All dependencies and infrastructure choices must be open source
2. **Test-driven development**: Tests before production code, always
3. **Self-hostable**: Works fully offline with Docker Compose, no external services required
4. **Single deployable**: One binary, one container, simple to operate
5. **No vendor lock-in**: Standard protocols (S3, CQL, HTTP) throughout

See [CLAUDE.md](../CLAUDE.md) for detailed development guidelines and TDD workflow.
