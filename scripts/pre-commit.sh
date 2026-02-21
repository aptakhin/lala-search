#!/bin/sh
# SPDX-License-Identifier: BSD-3-Clause
# Copyright (c) 2026 Aleksandr Ptakhin

# Pre-commit check script for LalaSearch
# Runs formatting checks, linting, and tests (including storage-dependent tests)
# Run this manually before committing: ./scripts/pre-commit.sh
#
# This script will automatically start required Docker services if not running.

set -e

echo "Running pre-commit checks for lala-agent..."

# Ensure we're in the project root for docker compose
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_ROOT/lala-agent"

# Check formatting
echo "1/4 Checking code formatting..."
cargo fmt --check
if [ $? -ne 0 ]; then
    echo "❌ Formatting check failed. Run 'cargo fmt' to fix."
    exit 1
fi
echo "✓ Formatting check passed"

# Run clippy
echo "2/4 Running clippy linter..."
cargo clippy -- -D warnings
if [ $? -ne 0 ]; then
    echo "❌ Clippy found issues. Fix them before committing."
    exit 1
fi
echo "✓ Clippy check passed"

# Run unit tests (fast, no external dependencies)
echo "3/4 Running unit tests..."
cargo test --lib
if [ $? -ne 0 ]; then
    echo "❌ Unit tests failed. Fix failing tests before committing."
    exit 1
fi
echo "✓ Unit tests passed"

# Start Docker services for storage-dependent tests
echo "4/4 Running storage-dependent tests..."
echo "    Starting Docker services (cassandra, seaweedfs, meilisearch)..."
cd "$PROJECT_ROOT"
docker compose up -d cassandra seaweedfs meilisearch cassandra-init

# Wait for cassandra-init to finish creating keyspaces
echo "    Waiting for services to be ready..."
echo "    Waiting for cassandra-init to complete schema creation..."
timeout=120
elapsed=0
while [ $elapsed -lt $timeout ]; do
    status=$(docker compose ps cassandra-init --format '{{.State}}' 2>/dev/null || echo "unknown")
    case "$status" in
        *exited*|*dead*)
            exit_code=$(docker compose ps cassandra-init --format '{{.ExitCode}}' 2>/dev/null || echo "1")
            if [ "$exit_code" = "0" ]; then
                echo "    cassandra-init completed successfully."
                break
            else
                echo "    ❌ cassandra-init failed with exit code $exit_code"
                docker compose logs cassandra-init
                exit 1
            fi
            ;;
        *)
            sleep 2
            elapsed=$((elapsed + 2))
            ;;
    esac
done
if [ $elapsed -ge $timeout ]; then
    echo "    ❌ Timed out waiting for cassandra-init (${timeout}s)"
    docker compose logs cassandra-init
    exit 1
fi

# Load environment variables from .env file for tests
if [ -f "$PROJECT_ROOT/.env" ]; then
    export $(grep -v '^#' "$PROJECT_ROOT/.env" | grep -v '^$' | xargs)
fi

cd "$PROJECT_ROOT/lala-agent"
cargo test --lib -- --ignored
if [ $? -ne 0 ]; then
    echo "❌ Storage-dependent tests failed."
    exit 1
fi
echo "✓ Storage-dependent tests passed"

echo "✅ All pre-commit checks passed!"
exit 0
