#!/bin/sh
# SPDX-License-Identifier: BSD-3-Clause
# Copyright (c) 2026 Aleksandr Ptakhin

# Pre-commit check script for LalaSearch
# Runs formatting checks, linting, and tests (including storage-dependent tests)
#
# Usage:
#   ./scripts/pre-commit.sh           # Auto-detect: Docker on Windows, local on Linux/macOS
#   ./scripts/pre-commit.sh --docker  # Force Docker mode (any OS)
#   ./scripts/pre-commit.sh --local   # Force local mode (requires Rust toolchain)
#
# On Windows (Git Bash / MSYS2), Docker mode is used by default to avoid
# PDB linker errors and other Windows-specific build issues. All checks run
# inside the lala-agent Docker container via `docker compose run`.
#
# This script will automatically start required Docker services if not running.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

# --- Determine run mode: docker or local ---
if [ "$1" = "--docker" ]; then
    USE_DOCKER=true
elif [ "$1" = "--local" ]; then
    USE_DOCKER=false
else
    # Auto-detect: use Docker on Windows (Git Bash / MSYS2 / Cygwin)
    case "$(uname -s)" in
        MINGW*|MSYS*|CYGWIN*) USE_DOCKER=true ;;
        *)                    USE_DOCKER=false ;;
    esac
fi

# --- Helper: start infrastructure and wait for postgres ---
start_infrastructure() {
    cd "$PROJECT_ROOT"

    echo "    Starting Docker services (postgres, seaweedfs, meilisearch)..."
    docker compose up -d postgres seaweedfs meilisearch seaweedfs-init

    echo "    Waiting for PostgreSQL to be ready..."
    max_wait=60
    waited=0
    while ! docker compose exec -T postgres pg_isready -U lalasearch -d lalasearch > /dev/null 2>&1; do
        sleep 2
        waited=$((waited + 2))
        if [ "$waited" -ge "$max_wait" ]; then
            echo "    ❌ Timed out waiting for PostgreSQL (${max_wait}s)"
            docker compose logs postgres
            exit 1
        fi
    done
    echo "    PostgreSQL is ready."
}

# --- Docker mode: run all checks inside lala-agent container ---
if [ "$USE_DOCKER" = true ]; then
    echo "Running pre-commit checks in Docker (via docker compose run)..."
    cd "$PROJECT_ROOT"

    # docker compose run starts dependencies automatically (postgres, seaweedfs,
    # meilisearch) and waits for health checks via the depends_on conditions
    # defined in docker-compose.yml.
    echo "Running fmt, clippy, and tests in lala-agent container..."
    docker compose run --rm lala-agent sh -c '
        # Source is volume-mounted read-only, so cargo cannot update mtimes.
        # Clear cached fingerprints to force recompilation against the latest code.
        rm -rf target/debug/.fingerprint/lala-agent-* \
               target/debug/deps/lala_agent-* \
               target/debug/lala-agent 2>/dev/null || true
        echo "1/3 Checking code formatting..." && \
        cargo fmt --check && \
        echo "✓ Formatting check passed" && \
        echo "2/3 Running clippy linter..." && \
        cargo clippy -- -D warnings && \
        echo "✓ Clippy check passed" && \
        echo "3/3 Running all tests (unit + storage-dependent)..." && \
        cargo run --release -- migrate && \
        cargo test --lib -- --include-ignored && \
        echo "✓ All tests passed"
    '

    echo "✅ All pre-commit checks passed!"
    exit 0
fi

# --- Local mode: run checks with local Rust toolchain ---
echo "Running pre-commit checks for lala-agent..."

cd "$PROJECT_ROOT/lala-agent"

# Check formatting
echo "1/3 Checking code formatting..."
cargo fmt --check
if [ $? -ne 0 ]; then
    echo "❌ Formatting check failed. Run 'cargo fmt' to fix."
    exit 1
fi
echo "✓ Formatting check passed"

# Run clippy
echo "2/3 Running clippy linter..."
cargo clippy -- -D warnings
if [ $? -ne 0 ]; then
    echo "❌ Clippy found issues. Fix them before committing."
    exit 1
fi
echo "✓ Clippy check passed"

# Start Docker services for storage-dependent tests
echo "3/3 Running all tests (unit + storage-dependent)..."
start_infrastructure

# Load environment variables from .env file for tests
if [ -f "$PROJECT_ROOT/.env" ]; then
    export $(grep -v '^#' "$PROJECT_ROOT/.env" | grep -v '^$' | xargs)
fi

cd "$PROJECT_ROOT/lala-agent"
cargo test --lib -- --include-ignored
if [ $? -ne 0 ]; then
    echo "❌ Tests failed. Fix failing tests before committing."
    exit 1
fi
echo "✓ All tests passed"

echo "✅ All pre-commit checks passed!"
exit 0
