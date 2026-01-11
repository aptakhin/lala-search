# LalaSearch

An ambitious open source distributed search engine built with Rust.

## Overview

LalaSearch implements a leader-follower agent architecture for distributed web crawling and indexing. See [docs/overview.md](docs/overview.md) for detailed architecture information.

## Project Structure

```
lalasearch/
├── docs/                    # Documentation
│   ├── overview.md         # Project vision and architecture
│   └── claude-guidelines.md # Development workflow and TDD guidelines
├── lala-agent/             # Core agent implementation
│   ├── src/
│   │   └── main.rs         # HTTP server with /version endpoint
│   ├── Cargo.toml          # Rust dependencies
│   └── .rustfmt.toml       # Code formatting config
└── scripts/
    └── pre-commit.sh       # Pre-commit validation script
```

## Getting Started

### Prerequisites

- Rust 1.70+ ([Install Rust](https://rustup.rs/))
- Cargo (comes with Rust)

### Running lala-agent

```bash
cd lala-agent
cargo run
```

The agent will start on `http://127.0.0.1:3000`

### Testing the Version Endpoint

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

- HTTP server with version endpoint
- Test-driven development workflow established
- Code quality tooling configured
- Build-time version extraction from Cargo.toml

## License

To be determined

