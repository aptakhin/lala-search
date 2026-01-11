#!/bin/sh
# SPDX-License-Identifier: BSD-3-Clause
# Copyright (c) 2026 Aleksandr Ptakhin

# Pre-commit check script for LalaSearch
# Runs formatting checks, linting, and tests
# Run this manually before committing: ./scripts/pre-commit.sh

set -e

echo "Running pre-commit checks for lala-agent..."

cd lala-agent

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

# Run tests
echo "3/3 Running tests..."
cargo test
if [ $? -ne 0 ]; then
    echo "❌ Tests failed. Fix failing tests before committing."
    exit 1
fi
echo "✓ All tests passed"

echo "✅ All pre-commit checks passed!"
exit 0
