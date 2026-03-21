#!/usr/bin/env bash
# SPDX-License-Identifier: BSD-3-Clause
# Copyright (c) 2026 Aleksandr Ptakhin
#
# Initialize Let's Encrypt certificates for LalaSearch.
#
# On first deploy, nginx needs certs to start the HTTPS server block,
# but certbot needs nginx to serve the ACME challenge. This script
# breaks the chicken-and-egg by creating a self-signed dummy cert first,
# starting nginx, then replacing it with a real Let's Encrypt cert.
#
# Usage (called by deploy.sh on the remote host):
#   ./init-letsencrypt.sh <domain> <email> <deploy_dir>
#
# If a valid Let's Encrypt cert already exists for the domain, this
# script does nothing (safe to re-run on every deploy).

set -euo pipefail

DOMAIN="$1"
EMAIL="$2"
DEPLOY_DIR="$3"
CERT_NAME="lalasearch"
COMPOSE="docker compose --env-file .env.prod -f docker-compose.prod.yml"

cd "$DEPLOY_DIR"

# Check if a real Let's Encrypt cert already exists
CERT_DIR="/etc/letsencrypt/live/${CERT_NAME}"
if docker run --rm -v lalasearch_certbot-certs:/etc/letsencrypt alpine \
    sh -c "test -f ${CERT_DIR}/fullchain.pem && test -f ${CERT_DIR}/privkey.pem" 2>/dev/null; then
    echo "Let's Encrypt cert for ${DOMAIN} already exists — skipping init."
    exit 0
fi

echo "==> No existing cert found. Initializing Let's Encrypt for ${DOMAIN}..."

# Step 1: Create a self-signed dummy cert so nginx can start
echo "  Creating dummy certificate..."
docker run --rm \
    -v lalasearch_certbot-certs:/etc/letsencrypt \
    alpine sh -c "
        apk add --no-cache openssl > /dev/null 2>&1
        mkdir -p /etc/letsencrypt/live/${CERT_NAME}
        openssl req -x509 -nodes -newkey rsa:2048 -days 1 \
            -keyout /etc/letsencrypt/live/${CERT_NAME}/privkey.pem \
            -out /etc/letsencrypt/live/${CERT_NAME}/fullchain.pem \
            -subj '/CN=localhost' 2>/dev/null
    "

# Step 2: Start nginx (it will use the dummy cert)
echo "  Starting nginx with dummy cert..."
$COMPOSE up -d lala-web

# Wait for nginx to be ready
for i in $(seq 1 10); do
    if curl -sf http://localhost/health > /dev/null 2>&1; then
        break
    fi
    if [[ $i -eq 10 ]]; then
        echo "Error: nginx did not start within 30s" >&2
        exit 1
    fi
    sleep 3
done

# Step 3: Request real cert from Let's Encrypt
echo "  Requesting Let's Encrypt certificate..."

# Remove dummy cert first — certbot needs the directory clean
docker run --rm \
    -v lalasearch_certbot-certs:/etc/letsencrypt \
    alpine sh -c "rm -rf /etc/letsencrypt/live/${CERT_NAME} /etc/letsencrypt/renewal/${CERT_NAME}.conf"

docker run --rm \
    -v lalasearch_certbot-certs:/etc/letsencrypt \
    -v lalasearch_certbot-webroot:/var/www/certbot \
    certbot/certbot certonly \
        --webroot \
        --webroot-path=/var/www/certbot \
        --cert-name "$CERT_NAME" \
        -d "$DOMAIN" \
        --email "$EMAIL" \
        --agree-tos \
        --no-eff-email \
        --non-interactive

# Step 4: Reload nginx to pick up the real cert
echo "  Reloading nginx with real certificate..."
docker exec lalasearch-web nginx -s reload

echo "==> Let's Encrypt certificate initialized for ${DOMAIN}."
