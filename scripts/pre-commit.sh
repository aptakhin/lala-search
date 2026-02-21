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

# Wait for services to be healthy
echo "    Waiting for services to be ready..."
docker compose exec -T cassandra cqlsh -e "SELECT now() FROM system.local" > /dev/null 2>&1 || sleep 30

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
