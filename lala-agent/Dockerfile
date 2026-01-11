# SPDX-License-Identifier: BSD-3-Clause
# Copyright (c) 2026 Aleksandr Ptakhin

# Single-stage Rust development image
FROM rust:1.83-bullseye

# Create app directory
WORKDIR /app

# Install curl for health checks early
RUN apt-get update && apt-get install -y curl && rm -rf /var/lib/apt/lists/*

# Install cargo-watch for hot-reload development (skip if incompatible)
# Note: cargo-watch may require nightly rust, skip for now
# RUN cargo install cargo-watch

# Copy Cargo files first for better caching
COPY Cargo.toml Cargo.lock ./
COPY build.rs ./

# Create dummy main.rs to build dependencies
RUN mkdir -p src && \
    echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src

# Copy source code
COPY src ./src
COPY tests ./tests

# Build the application
RUN cargo build --release

# Expose the application port
EXPOSE 3000

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:3000/version || exit 1

# Default command runs the release binary
# For development, use: cargo run (without hot-reload)
CMD ["./target/release/lala-agent"]
