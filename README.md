# LalaSearch

[![Build & Test](https://github.com/aptakhin/lala-search/actions/workflows/ci.yml/badge.svg)](https://github.com/aptakhin/lala-search/actions/workflows/ci.yml)
[![E2E Tests](https://github.com/aptakhin/lala-search/actions/workflows/e2e.yml/badge.svg)](https://github.com/aptakhin/lala-search/actions/workflows/e2e.yml)

Open source self-hosted search for your private web. Index and search internal tools, knowledge bases, and communication systems — all running on your own infrastructure.

## Overview

LalaSearch lets you build a unified search index over your private and internal systems. It crawls authenticated web applications, indexes their content, and exposes a fast full-text search API. No data leaves your infrastructure.

Two editions share the same codebase:

| Edition | Description |
|---------|-------------|
| **Community** | Single-tenant, self-hosted, fully open source. Deploy on your own server. |
| **SaaS** | Multi-tenant hosted version. Tenant isolation via PostgreSQL Row-Level Security. May move to a separate private repository as the offering matures. |

## What It Does

- Crawls and indexes private web systems — internal wikis, knowledge bases, communication tools, project trackers
- Provides a full-text search API over all indexed content
- Respects `robots.txt` and per-domain crawl rules
- Stores raw content in S3-compatible object storage (SeaweedFS by default)
- Indexes content in Meilisearch for sub-millisecond search queries
- Supports magic-link authentication and organization-based access control

## Project Structure

```
lalasearch/
├── docs/                           # Documentation
│   ├── overview.md                # Project vision and architecture
│   ├── api.md                     # API reference with curl examples
│   ├── deployment.md              # Production deployment guide (Linux VM)
│   ├── vm-metrics.md              # Host VM metrics via Grafana Alloy
│   ├── docker.md                  # Docker setup and usage guide
│   ├── versioning.md              # Version management
│   └── multi-tenancy.md           # Multi-tenancy architecture
├── lala-agent/                    # Core agent implementation
│   ├── src/
│   │   ├── main.rs                # HTTP server entry point
│   │   ├── lib.rs                 # Library root
│   │   ├── models/                # Data models
│   │   │   ├── agent.rs          # AgentMode enum (worker/manager/all)
│   │   │   ├── deployment.rs     # DeploymentMode enum (single/multi tenant)
│   │   │   ├── db.rs             # PostgreSQL row types
│   │   │   ├── crawler.rs        # Crawler request/result models
│   │   │   ├── queue.rs          # Crawl queue entry model
│   │   │   ├── search.rs         # Search request/response models
│   │   │   ├── onboarding.rs     # Onboarding/recent pages models
│   │   │   ├── settings.rs       # Settings model
│   │   │   ├── storage.rs        # S3 storage models
│   │   │   └── version.rs        # Version response model
│   │   └── services/              # Business logic
│   │       ├── crawler.rs        # Web crawler with robots.txt support
│   │       ├── db.rs             # PostgreSQL client (sqlx + PgPool)
│   │       ├── queue_processor.rs # Queue processing and crawl pipeline
│   │       ├── search.rs         # Meilisearch client
│   │       └── storage.rs        # S3 storage client with gzip compression
│   ├── tests/                     # Integration tests
│   │   ├── crawler_integration_test.rs
│   │   └── queue_processor_integration_test.rs
│   ├── Dockerfile                 # Development container image
│   ├── Dockerfile.prod            # Production multi-stage image (~100MB)
│   ├── Cargo.toml                 # Rust dependencies
│   └── build.rs                   # Build-time version extraction
├── docker/                        # Docker configuration
├── .github/workflows/
│   ├── ci.yml                    # Build & Test pipeline (fmt, clippy, unit, storage, integration)
│   ├── e2e.yml                   # E2E Test pipeline (Docker Compose + Playwright)
│   └── publish.yml               # Publish Docker images to GHCR on version tags
├── docker-compose.yml             # Development multi-container setup
├── docker-compose.prod.yml        # Production deployment (pre-built images)
├── .env.example                   # Development environment template
├── .env.prod.example              # Production environment template
└── scripts/
    └── pre-commit.sh              # Pre-commit validation script
```

## Getting Started

### Option 1: Docker (Recommended)

Run LalaSearch with Docker Compose (includes PostgreSQL, Meilisearch, and SeaweedFS):

```bash
# Copy environment configuration
cp .env.example .env
# Edit .env with your SMTP settings and other configuration

# Start all services
docker compose up -d --build

# Check status
docker compose ps

# View logs
docker compose logs -f

# Test the agent
curl http://localhost:3000/version
```

See [docs/docker.md](docs/docker.md) for detailed Docker setup and usage.

### Production Deployment

Deploy LalaSearch on a Linux VM (Debian/Ubuntu) using pre-built Docker images:

```bash
# Download deployment files
mkdir -p lalasearch/docker/seaweedfs && cd lalasearch
REPO="https://raw.githubusercontent.com/aptakhin/lala-search/main"
curl -fsSLO "$REPO/docker-compose.prod.yml"
curl -fsSLO "$REPO/.env.prod.example"
curl -fsSL "$REPO/docker/seaweedfs/s3.json" -o docker/seaweedfs/s3.json

# Configure (change all CHANGE_ME values!)
cp .env.prod.example .env.prod
nano .env.prod

# Start
docker compose -f docker-compose.prod.yml up -d
```

See [docs/deployment.md](docs/deployment.md) for the full guide including HTTPS setup, backups, and troubleshooting.

### Option 2: Local Development

#### Prerequisites

- Rust 1.70+ ([Install Rust](https://rustup.rs/))
- Cargo (comes with Rust)

#### Running lala-agent

```bash
cd lala-agent
cargo run
```

The agent will start on `http://127.0.0.1:3000`

See [docs/api.md](docs/api.md) for complete API reference with curl examples.

## Development

This project follows Test-Driven Development (TDD). See [CLAUDE.md](CLAUDE.md) for detailed development workflow.

### First-Time Setup

After cloning, install the git pre-commit hook to automatically run quality checks:

```bash
# Create a hook that delegates to scripts/pre-commit.sh
printf '#!/bin/sh\nexec "$(git rev-parse --show-toplevel)/scripts/pre-commit.sh"\n' > .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
```

On Windows (Git Bash), the pre-commit script automatically runs all checks inside Docker via `docker compose run lala-agent` to avoid PDB linker errors and other Windows-specific build issues.

You can also force a specific mode:
```bash
./scripts/pre-commit.sh --docker  # Force Docker mode (any OS)
./scripts/pre-commit.sh --local   # Force local Rust toolchain
```

### Running Tests

```bash
# Unit and integration tests
cd lala-agent
cargo test

# End-to-end tests (requires Docker services running)
./tests/e2e/run_tests.sh
```

See [tests/e2e/README.md](tests/e2e/README.md) for more testing options.

### Code Quality

```bash
cd lala-agent
cargo fmt          # Format code
cargo clippy -- -D warnings   # Lint (zero warnings policy)
./scripts/pre-commit.sh       # Full pre-commit check
```

## S3 Storage Configuration

LalaSearch stores raw crawled content in S3-compatible object storage.

### Supported Providers

- **SeaweedFS** (included in Docker Compose for local development)
- AWS S3
- Any S3-compatible storage

### Configuration

| Variable | Description | Example |
|----------|-------------|---------|
| `S3_ENDPOINT` | S3 endpoint URL | `http://seaweedfs:8333` |
| `S3_REGION` | AWS region (optional for SeaweedFS) | `us-east-1` |
| `S3_BUCKET` | Bucket name | `lalasearch-content` |
| `S3_ACCESS_KEY` | Access key ID | `any` |
| `S3_SECRET_KEY` | Secret access key | `any` |
| `S3_COMPRESS_CONTENT` | Enable gzip compression | `true` |
| `S3_COMPRESS_MIN_SIZE` | Min size for compression (bytes) | `1024` |

Storage details:
- Content stored with UUID v7 keys (time-ordered, sortable)
- Files named `{uuid}.html` or `{uuid}.html.gz` based on compression
- PostgreSQL stores `storage_id` (UUID) and `storage_compression` type

## Deployment Modes

Controlled by the `DEPLOYMENT_MODE` environment variable:

| Mode | Value | Description |
|------|-------|-------------|
| Single-tenant | `single_tenant` | Self-hosted Community edition (default) |
| Multi-tenant | `multi_tenant` | SaaS hosted version — tenant isolation via PostgreSQL RLS |

The core crawling, storage, search, and queue logic is identical between both modes. See [docs/multi-tenancy.md](docs/multi-tenancy.md) for the full architecture.

> **Note**: Multi-tenant and SaaS-specific code is currently open source in this repository. This may change — billing and tenant provisioning code may move to a private repository as the hosted offering matures. The Community (single-tenant) edition will always remain fully open source.

## Current Status

**Implemented:**
- HTTP server with version and health endpoints
- Web crawler with robots.txt compliance
- PostgreSQL for crawl metadata, auth, and tenant management
- Crawl queue management and processing pipeline
- S3-compatible storage for crawled HTML content with gzip compression
- Meilisearch integration for full-text search
- Magic-link authentication and session management
- Organization-based access control (owner/admin/member roles)
- Onboarding page for first-time tenant setup with live crawl console
- Single-tenant / multi-tenant deployment modes with PostgreSQL RLS
- Docker and Docker Compose setup with proper startup ordering
- Test-driven development workflow and pre-commit hooks
- GitHub Actions CI/CD (Build & Test + E2E pipelines)

**Planned:**
- Integration connectors for communication and knowledge base systems
- Connector configuration API (credentials, crawl schedule, scope)
- Incremental re-crawl and change detection
- Search result ranking tuned for internal content

## Versioning

LalaSearch uses semantic versioning:
- **MAJOR.MINOR**: Manually set in `lala-agent/Cargo.toml`
- **PATCH**: Auto-generated from CI/CD pipeline run number

See [docs/versioning.md](docs/versioning.md) for details.

## License

BSD 3-Clause License - see [LICENSE](LICENSE) file for details.

Copyright (c) 2026, Aleksandr Ptakhin
