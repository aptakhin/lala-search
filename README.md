# LalaSearch

An ambitious open source distributed search engine built with Rust.

## Overview

LalaSearch implements a leader-follower agent architecture for distributed web crawling and indexing. See [docs/overview.md](docs/overview.md) for detailed architecture information.

## Project Structure

```
lalasearch/
├── docs/                           # Documentation
│   ├── overview.md                # Project vision and architecture
│   ├── claude-guidelines.md       # Development workflow and TDD guidelines
│   ├── docker.md                  # Docker setup and usage guide
│   └── versioning.md              # Version management
├── lala-agent/                    # Core agent implementation
│   ├── src/
│   │   ├── main.rs                # HTTP server entry point
│   │   ├── lib.rs                 # Library root
│   │   ├── models/                # Data models
│   │   │   ├── version.rs        # Version response model
│   │   │   └── crawler.rs        # Crawler request/result models
│   │   └── services/              # Business logic
│   │       └── crawler.rs        # Web crawler with robots.txt support
│   ├── tests/                     # Integration tests
│   │   └── crawler_integration_test.rs
│   ├── Dockerfile                 # Container image definition
│   ├── Cargo.toml                 # Rust dependencies
│   └── build.rs                   # Build-time version extraction
├── docker/                        # Docker configuration
│   └── scylla/
│       └── schema.cql             # ScyllaDB database schema
├── docker-compose.yml             # Multi-container setup
└── scripts/
    └── pre-commit.sh              # Pre-commit validation script
```

## Getting Started

### Option 1: Docker (Recommended)

Run LalaSearch with Docker Compose (includes ScyllaDB):

```bash
# Start all services
docker-compose up -d

# Check status
docker-compose ps

# View logs
docker-compose logs -f

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

Expected response:
```json
{
  "agent": "lala-agent",
  "version": "0.1.0"
}
```

## Development

This project follows Test-Driven Development (TDD). See [docs/claude-guidelines.md](docs/claude-guidelines.md) for detailed development workflow.

### First-Time Setup

After cloning, install the git pre-commit hook to automatically run quality checks:

```bash
# Copy the pre-commit hook
cp scripts/pre-commit.sh .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
```

This hook will automatically run before every commit to ensure code quality.

### Running Tests

```bash
cd lala-agent
cargo test
```

### Code Quality Checks

The pre-commit hook automatically runs before each commit. To run manually:

```bash
# From repository root
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

## Current Status

✅ **Implemented:**
- HTTP server with version endpoint
- Web crawler with robots.txt compliance
- Modular architecture (models, services, handlers)
- Docker and Docker Compose setup
- ScyllaDB integration for crawl metadata
- Test-driven development workflow
- Code quality tooling and pre-commit hooks
- Build-time version extraction

🚧 **In Progress:**
- ScyllaDB client integration in Rust
- Crawl queue management
- Distributed worker coordination

## License

BSD 3-Clause License - see [LICENSE](LICENSE) file for details.

Copyright (c) 2026, Aleksandr Ptakhin

