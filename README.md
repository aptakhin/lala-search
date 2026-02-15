# LalaSearch

An ambitious open source distributed search engine built with Rust.

## Overview

LalaSearch implements a leader-follower agent architecture for distributed web crawling and indexing. See [docs/overview.md](docs/overview.md) for detailed architecture information.

## Project Structure

```
lalasearch/
├── docs/                           # Documentation
│   ├── overview.md                # Project vision and architecture
│   ├── api.md                     # API reference with curl examples
│   ├── docker.md                  # Docker setup and usage guide
│   ├── versioning.md              # Version management
│   └── multi-tenancy.md           # Multi-tenancy architecture decisions
├── lala-agent/                    # Core agent implementation
│   ├── src/
│   │   ├── main.rs                # HTTP server entry point
│   │   ├── lib.rs                 # Library root
│   │   ├── models/                # Data models
│   │   │   ├── agent.rs          # AgentMode enum (worker/manager/all)
│   │   │   ├── deployment.rs     # DeploymentMode enum (single/multi tenant)
│   │   │   ├── db.rs             # Cassandra row types
│   │   │   ├── crawler.rs        # Crawler request/result models
│   │   │   ├── queue.rs          # Crawl queue entry model
│   │   │   ├── search.rs         # Search request/response models
│   │   │   ├── settings.rs       # Settings model
│   │   │   ├── storage.rs        # S3 storage models
│   │   │   └── version.rs        # Version response model
│   │   └── services/              # Business logic
│   │       ├── crawler.rs        # Web crawler with robots.txt support
│   │       ├── db.rs             # Cassandra client (fully qualified table names)
│   │       ├── queue_processor.rs # Queue processing and crawl pipeline
│   │       ├── search.rs         # Meilisearch client
│   │       └── storage.rs        # S3 storage client with gzip compression
│   ├── tests/                     # Integration tests
│   │   ├── crawler_integration_test.rs
│   │   └── queue_processor_integration_test.rs
│   ├── Dockerfile                 # Container image definition
│   ├── Cargo.toml                 # Rust dependencies
│   └── build.rs                   # Build-time version extraction
├── docker/                        # Docker configuration
│   └── cassandra/
│       ├── schema.cql             # Tenant keyspace schema (lalasearch_default)
│       └── schema_system.cql      # System keyspace schema (lalasearch_system)
├── docker-compose.yml             # Multi-container setup
├── .env.example                   # Environment variables template
└── scripts/
    └── pre-commit.sh              # Pre-commit validation script
```

## Getting Started

### Option 1: Docker (Recommended)

Run LalaSearch with Docker Compose (includes Apache Cassandra):

```bash
# Copy environment configuration
cp .env.example .env
# Edit .env if needed for your local setup

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

#### Testing the Version Endpoint

```bash
curl http://127.0.0.1:3000/version
```

See [docs/api.md](docs/api.md) for complete API reference with curl examples.

## Development

This project follows Test-Driven Development (TDD). See [CLAUDE.md](CLAUDE.md) for detailed development workflow.

### Manual Testing with Crawl Queue

See [docs/api.md](docs/api.md) for API examples including:
- Adding/removing allowed domains
- Enabling/disabling crawling
- Adding URLs to the queue
- Searching indexed documents

The lala-agent will automatically pick up entries from the queue and process them. Monitor logs with:

```bash
docker compose logs -f lala-agent
```

#### Viewing Queue Status via Database

You can also query the database directly to see queue and crawled page status:

```bash
# Connect to Cassandra via Docker
docker exec -it lalasearch-cassandra cqlsh

# View the queue (fully qualified keyspace.table)
SELECT * FROM lalasearch_default.crawl_queue;

# View crawled pages (after the agent processes the queue)
SELECT * FROM lalasearch_default.crawled_pages;

# Check for a specific crawled page
SELECT * FROM lalasearch_default.crawled_pages WHERE domain = 'en.wikipedia.org' AND url_path = '/wiki/Main_Page';

# View tenant registry
SELECT * FROM lalasearch_system.tenants;
```

### First-Time Setup

After cloning, install the git pre-commit hook to automatically run quality checks:

```bash
# Copy the pre-commit hook
cp scripts/pre-commit.sh .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
```

This hook will automatically run before every commit to ensure code quality.

### Running Tests

#### Unit and Integration Tests

```bash
cd lala-agent
cargo test
```

#### End-to-End Tests

```bash
# Automated runner (recommended for CI/CD)
./tests/e2e/run_tests.sh
```

The E2E test runner will:
- Start Docker Compose services if needed
- Install Python dependencies with uv
- Run full pipeline tests (queue → crawl → index → search)

See [tests/e2e/README.md](tests/e2e/README.md) for more testing options.

### Code Quality Checks

The pre-commit hook automatically runs before each commit. To run manually:

```bash
# From repository root
chmod +x ./scripts/pre-commit.sh
./scripts/pre-commit.sh
```

Or run checks individually:

```bash
cd lala-agent

# Format code
cargo fmt

# Check formatting
cargo fmt --check

# Run linter
cargo clippy -- -D warnings

# Run tests
cargo test
```

## Versioning

LalaSearch uses semantic versioning with a hybrid approach:
- **MAJOR.MINOR**: Manually set in `lala-agent/Cargo.toml`
- **PATCH**: Auto-generated from CI/CD pipeline run number (future)

See [docs/versioning.md](docs/versioning.md) for detailed version management.

## S3 Storage Configuration

LalaSearch can store raw HTML content in S3-compatible storage for archival and replay purposes.

### Supported Providers

- **MinIO** (included in Docker Compose for local development)
- AWS S3
- DigitalOcean Spaces
- Wasabi
- Any S3-compatible storage

### Configuration

Set the following environment variables in your `.env` file:

| Variable | Description | Example |
|----------|-------------|---------|
| `S3_ENDPOINT` | S3 endpoint URL | `http://minio:9000` |
| `S3_REGION` | AWS region (optional for MinIO) | `us-east-1` |
| `S3_BUCKET` | Bucket name | `lalasearch-content` |
| `S3_ACCESS_KEY` | Access key ID | `minioadmin` |
| `S3_SECRET_KEY` | Secret access key | `minioadmin` |
| `S3_COMPRESS_CONTENT` | Enable gzip compression | `true` |
| `S3_COMPRESS_MIN_SIZE` | Min size for compression (bytes) | `1024` |

### Storage Details

- Content is stored with UUID v7 keys (time-ordered, sortable)
- Files are named `{uuid}.html` or `{uuid}.html.gz` (based on compression)
- Cassandra stores both `storage_id` (UUID) and `storage_compression` (0=none, 1=gzip)
- Compression type determines the correct S3 object key for retrieval
- No trial-and-error lookups - compression metadata ensures single S3 request

## Deployment Modes

LalaSearch supports two deployment modes controlled by the `DEPLOYMENT_MODE` environment variable:

| Mode | Value | Description |
|------|-------|-------------|
| Single-tenant | `single_tenant` | Self-hosted open source installation (default) |
| Multi-tenant | `multi_tenant` | SaaS/hosted version — one Cassandra keyspace per customer |

In single-tenant mode there is one tenant (`default`) and one data keyspace (`lalasearch_default`). The multi-tenant mode is the same codebase — only the auth middleware changes to route requests to per-tenant keyspaces.

> **Note**: Multi-tenant features (tenant management, billing, payments) may be moved to a separate proprietary repository in the future as part of an open-core model. The single-tenant self-hosted version will always remain open source.

See [docs/multi-tenancy.md](docs/multi-tenancy.md) for the full architecture.

## Current Status

✅ **Implemented:**
- HTTP server with version and health endpoints
- Web crawler with robots.txt compliance
- Apache Cassandra for crawl metadata storage (fully qualified table names)
- Crawl queue management and distributed queue processing
- S3-compatible storage for crawled HTML content with gzip compression
- Meilisearch integration for full-text search
- Single-tenant / multi-tenant deployment modes
- System keyspace (`lalasearch_system`) with global tenant registry
- Docker and Docker Compose setup with proper startup ordering
- Modular architecture (models, services, handlers)
- Test-driven development workflow
- Code quality tooling and pre-commit hooks
- Build-time version extraction

🚧 **In Progress:**
- Distributed worker coordination (leader-follower)
- Multi-tenant auth middleware and tenant provisioning API

## License

BSD 3-Clause License - see [LICENSE](LICENSE) file for details.

Copyright (c) 2026, Aleksandr Ptakhin

