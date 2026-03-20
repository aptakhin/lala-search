#!/usr/bin/env bash
# SPDX-License-Identifier: BSD-3-Clause
# Copyright (c) 2026 Aleksandr Ptakhin
#
# Deploy LalaSearch to a remote Linux (Debian/Ubuntu) server via SSH.
#
# Reads secrets from environment variables (set via GitHub Actions secrets
# or export manually). SSHs to the target machine, ensures Docker is
# installed, downloads deployment files, generates .env.prod from secrets,
# and brings the stack up.
#
# Usage:
#   # Set required env vars, then:
#   ./scripts/deploy.sh
#
# Required environment variables:
#   DEPLOY_HOST          - SSH host (IP or hostname)
#   DEPLOY_USER          - SSH user (must have sudo access)
#   DEPLOY_SSH_KEY       - Private SSH key contents (not a path)
#   POSTGRES_PASSWORD    - PostgreSQL password
#   S3_ACCESS_KEY        - SeaweedFS S3 access key
#   S3_SECRET_KEY        - SeaweedFS S3 secret key
#
# Optional environment variables:
#   DEPLOY_PORT          - SSH port (default: 22)
#   DEPLOY_DIR           - Remote install directory (default: /opt/lalasearch)
#   APP_BASE_URL         - Public URL (default: http://$DEPLOY_HOST)
#   SMTP_HOST            - SMTP server (default: postfix)
#   SMTP_PORT            - SMTP port (default: 25)
#   SMTP_USERNAME        - SMTP username (default: empty)
#   SMTP_PASSWORD        - SMTP password (default: empty)
#   SMTP_TLS             - SMTP TLS enabled (default: false)
#   SMTP_FROM_EMAIL      - Sender email (default: noreply@$DEPLOY_HOST)
#   SMTP_FROM_NAME       - Sender name (default: LalaSearch)
#   IMAGE_TAG            - Docker image tag (default: latest)

set -euo pipefail

# ── Validate required variables ──────────────────────────────────────────────

missing=()
for var in DEPLOY_HOST DEPLOY_USER DEPLOY_SSH_KEY POSTGRES_PASSWORD S3_ACCESS_KEY S3_SECRET_KEY; do
    if [[ -z "${!var:-}" ]]; then
        missing+=("$var")
    fi
done

if [[ ${#missing[@]} -gt 0 ]]; then
    echo "Error: missing required environment variables: ${missing[*]}" >&2
    echo "See script header for the full list." >&2
    exit 1
fi

# ── Defaults ─────────────────────────────────────────────────────────────────

DEPLOY_PORT="${DEPLOY_PORT:-22}"
DEPLOY_DIR="${DEPLOY_DIR:-/opt/lalasearch}"
APP_BASE_URL="${APP_BASE_URL:-http://${DEPLOY_HOST}}"
SMTP_HOST="${SMTP_HOST:-postfix}"
SMTP_PORT="${SMTP_PORT:-25}"
SMTP_USERNAME="${SMTP_USERNAME:-}"
SMTP_PASSWORD="${SMTP_PASSWORD:-}"
SMTP_TLS="${SMTP_TLS:-false}"
SMTP_FROM_EMAIL="${SMTP_FROM_EMAIL:-noreply@${DEPLOY_HOST}}"
SMTP_FROM_NAME="${SMTP_FROM_NAME:-LalaSearch}"
IMAGE_TAG="${IMAGE_TAG:-latest}"
REPO_RAW="https://raw.githubusercontent.com/aptakhin/lala-search/main"

# ── Set up SSH ───────────────────────────────────────────────────────────────

SSH_KEY_FILE="$(mktemp)"
trap 'rm -f "$SSH_KEY_FILE"' EXIT
echo "$DEPLOY_SSH_KEY" > "$SSH_KEY_FILE"
chmod 600 "$SSH_KEY_FILE"

SSH_OPTS="-o StrictHostKeyChecking=accept-new -o BatchMode=yes -p ${DEPLOY_PORT} -i ${SSH_KEY_FILE}"

ssh_cmd() {
    # shellcheck disable=SC2086
    ssh $SSH_OPTS "${DEPLOY_USER}@${DEPLOY_HOST}" "$@"
}

echo "==> Deploying LalaSearch to ${DEPLOY_USER}@${DEPLOY_HOST}:${DEPLOY_PORT}"
echo "    Remote directory: ${DEPLOY_DIR}"
echo "    Image tag: ${IMAGE_TAG}"

# ── Step 1: Ensure Docker is installed ───────────────────────────────────────

echo "==> Checking Docker installation..."

ssh_cmd bash -s <<'REMOTE_DOCKER'
set -euo pipefail
if command -v docker &>/dev/null && docker compose version &>/dev/null; then
    echo "Docker and Compose plugin already installed."
else
    echo "Installing Docker..."
    curl -fsSL https://get.docker.com | sh
    sudo usermod -aG docker "$USER"
    echo "Docker installed. Note: group change takes effect on next login."
fi
REMOTE_DOCKER

# ── Step 2: Create directory structure and download deployment files ──────────

echo "==> Downloading deployment files..."

ssh_cmd bash -s -- "$DEPLOY_DIR" "$REPO_RAW" <<'REMOTE_DOWNLOAD'
set -euo pipefail
DEPLOY_DIR="$1"
REPO_RAW="$2"

sudo mkdir -p "${DEPLOY_DIR}/docker/seaweedfs"
sudo chown -R "$USER:$USER" "$DEPLOY_DIR"

cd "$DEPLOY_DIR"

echo "Downloading docker-compose.prod.yml..."
curl -fsSL "$REPO_RAW/docker-compose.prod.yml" -o docker-compose.prod.yml

echo "Downloading docker/seaweedfs/s3.json..."
curl -fsSL "$REPO_RAW/docker/seaweedfs/s3.json" -o docker/seaweedfs/s3.json

# Verify all required files exist and are non-empty
failed=()
for f in docker-compose.prod.yml docker/seaweedfs/s3.json; do
    if [[ ! -s "$f" ]]; then
        failed+=("$f")
    fi
done

if [[ ${#failed[@]} -gt 0 ]]; then
    echo "Error: failed to download deployment files: ${failed[*]}" >&2
    echo "Check that the repository URL is correct: $REPO_RAW" >&2
    exit 1
fi

echo "Deployment files downloaded."
REMOTE_DOWNLOAD

# ── Step 3: Generate .env.prod from secrets ──────────────────────────────────

echo "==> Writing .env.prod..."

# Build the env file content locally, send it over SSH.
# This avoids any escaping issues with special characters in passwords.
ENV_CONTENT="$(cat <<ENVEOF
# LalaSearch Production Environment — generated by deploy.sh
# $(date -u '+%Y-%m-%d %H:%M:%S UTC')

# === Agent Configuration ===
AGENT_MODE=all
DEPLOYMENT_MODE=single_tenant
ENVIRONMENT=prod
RUST_LOG=info

# === Database Configuration (PostgreSQL) ===
POSTGRES_USER=lalasearch
POSTGRES_PASSWORD=${POSTGRES_PASSWORD}
DATABASE_URL=postgres://lalasearch:${POSTGRES_PASSWORD}@postgres:5432/lalasearch

# === Search Engine Configuration (Meilisearch) ===
MEILISEARCH_HOST=meilisearch:7700

# === Crawler Configuration ===
QUEUE_POLL_INTERVAL_SECS=5
USER_AGENT=LalaSearchBot/0.1

# === S3 Storage Configuration (SeaweedFS) ===
S3_ENDPOINT=http://seaweedfs:8333
S3_REGION=us-east-1
S3_BUCKET=lalasearch-content
S3_ACCESS_KEY=${S3_ACCESS_KEY}
S3_SECRET_KEY=${S3_SECRET_KEY}
S3_COMPRESS_CONTENT=true
S3_COMPRESS_MIN_SIZE=1024

# === Authentication Configuration ===
SMTP_HOST=${SMTP_HOST}
SMTP_PORT=${SMTP_PORT}
SMTP_USERNAME=${SMTP_USERNAME}
SMTP_PASSWORD=${SMTP_PASSWORD}
SMTP_TLS=${SMTP_TLS}
SMTP_FROM_EMAIL=${SMTP_FROM_EMAIL}
SMTP_FROM_NAME=${SMTP_FROM_NAME}

APP_BASE_URL=${APP_BASE_URL}
SESSION_MAX_AGE_DAYS=365
MAGIC_LINK_EXPIRY_MINUTES=15
INVITATION_EXPIRY_DAYS=7
ENVEOF
)"

# Write via SSH stdin to avoid password chars being interpreted by the shell
echo "$ENV_CONTENT" | ssh_cmd "cat > ${DEPLOY_DIR}/.env.prod"

# ── Step 4: Pin image tag in compose file if not 'latest' ───────────────────

if [[ "$IMAGE_TAG" != "latest" ]]; then
    echo "==> Pinning images to tag: ${IMAGE_TAG}"
    ssh_cmd bash -s -- "$DEPLOY_DIR" "$IMAGE_TAG" <<'REMOTE_PIN'
set -euo pipefail
DEPLOY_DIR="$1"
TAG="$2"
cd "$DEPLOY_DIR"
sed -i "s|ghcr.io/aptakhin/lala-search/lala-agent:latest|ghcr.io/aptakhin/lala-search/lala-agent:${TAG}|g" docker-compose.prod.yml
sed -i "s|ghcr.io/aptakhin/lala-search/lala-web:latest|ghcr.io/aptakhin/lala-search/lala-web:${TAG}|g" docker-compose.prod.yml
REMOTE_PIN
fi

# ── Step 5: Pull images and start the stack ──────────────────────────────────

echo "==> Pulling images and starting the stack..."

ssh_cmd bash -s -- "$DEPLOY_DIR" <<'REMOTE_UP'
set -euo pipefail
DEPLOY_DIR="$1"
cd "$DEPLOY_DIR"

docker compose --env-file .env.prod -f docker-compose.prod.yml pull
docker compose --env-file .env.prod -f docker-compose.prod.yml up -d
REMOTE_UP

# ── Step 6: Wait for health and verify ───────────────────────────────────────

echo "==> Waiting for services to become healthy..."

ssh_cmd bash -s -- "$DEPLOY_DIR" <<'REMOTE_VERIFY'
set -euo pipefail
DEPLOY_DIR="$1"
cd "$DEPLOY_DIR"

# Wait up to 120 seconds for lala-agent to become healthy
for i in $(seq 1 24); do
    status=$(docker inspect --format='{{.State.Health.Status}}' lalasearch-agent 2>/dev/null || echo "not_found")
    if [[ "$status" == "healthy" ]]; then
        echo "lala-agent is healthy!"
        break
    fi
    if [[ $i -eq 24 ]]; then
        echo "Error: lala-agent did not become healthy within 120s" >&2
        docker compose --env-file .env.prod -f docker-compose.prod.yml ps
        docker compose --env-file .env.prod -f docker-compose.prod.yml logs --tail=30 lala-agent
        exit 1
    fi
    echo "  Waiting... ($status) [${i}/24]"
    sleep 5
done

echo ""
echo "Service status:"
docker compose --env-file .env.prod -f docker-compose.prod.yml ps

echo ""
echo "Version:"
curl -sf http://localhost:3000/version || echo "(agent API not reachable on localhost)"
REMOTE_VERIFY

echo ""
echo "==> Deployment complete!"
echo "    App: ${APP_BASE_URL}"
echo "    API: http://${DEPLOY_HOST}:3000/version (localhost only on remote)"
