# Deploying LalaSearch on Linux (Debian/Ubuntu)

Self-hosted deployment using Docker Compose with pre-built images from GitHub Container Registry.

## Prerequisites

A Debian/Ubuntu server with:
- 2+ GB RAM (4 GB recommended)
- 20+ GB disk space
- Docker Engine and Docker Compose plugin

### Install Docker

```bash
# Install Docker (official script)
curl -fsSL https://get.docker.com | sh

# Add your user to the docker group (log out and back in after)
sudo usermod -aG docker $USER

# Verify
docker compose version
```

## Quick Start

### 1. Download deployment files

```bash
mkdir -p lalasearch/docker/postgres lalasearch/docker/seaweedfs
cd lalasearch

# Download from the latest release
REPO="https://raw.githubusercontent.com/aptakhin/lala-search/main"
curl -fsSLO "$REPO/docker-compose.prod.yml"
curl -fsSLO "$REPO/.env.prod.example"
curl -fsSL "$REPO/docker/postgres/schema.sql" -o docker/postgres/schema.sql
curl -fsSL "$REPO/docker/seaweedfs/s3.json" -o docker/seaweedfs/s3.json
```

### 2. Configure environment

```bash
cp .env.prod.example .env.prod
```

Edit `.env.prod` and change **all** `CHANGE_ME` values:

```bash
# Generate strong passwords (run once per value)
openssl rand -base64 32
```

At minimum, update:
- `POSTGRES_PASSWORD` and the matching password in `DATABASE_URL`
- `S3_ACCESS_KEY` and `S3_SECRET_KEY`
- `SMTP_*` settings (for magic-link authentication emails)
- `APP_BASE_URL` (your public URL, e.g., `https://search.example.com`)

### 3. Start the stack

```bash
docker compose -f docker-compose.prod.yml up -d
```

First start pulls all images and initializes databases. This takes a few minutes.

### 4. Verify

```bash
# Check all services are healthy
docker compose -f docker-compose.prod.yml ps

# Test the API
curl http://localhost:3000/version

# Test the frontend
curl -I http://localhost:80
```

## Configuration Reference

| Variable | Description | Example |
|----------|-------------|---------|
| `ENVIRONMENT` | Runtime mode (`dev` or `prod`) | `prod` |
| `AGENT_MODE` | `worker`, `manager`, or `all` | `all` |
| `DEPLOYMENT_MODE` | `single_tenant` or `multi_tenant` | `single_tenant` |
| `POSTGRES_PASSWORD` | Database password | (generated) |
| `DATABASE_URL` | Full PostgreSQL connection URL | `postgres://lalasearch:PASSWORD@postgres:5432/lalasearch` |
| `S3_ACCESS_KEY` | SeaweedFS S3 access key | (generated) |
| `S3_SECRET_KEY` | SeaweedFS S3 secret key | (generated) |
| `SMTP_HOST` | SMTP server for auth emails | `smtp.mailgun.org` |
| `APP_BASE_URL` | Public frontend URL | `https://search.example.com` |

See [.env.prod.example](../.env.prod.example) for the full list with descriptions.

## HTTPS with a Reverse Proxy

The frontend listens on port 80. For production, put a reverse proxy in front for TLS termination.

### Option A: Caddy (recommended — automatic HTTPS)

Install Caddy:
```bash
sudo apt install -y debian-keyring debian-archive-keyring apt-transport-https
curl -1sLf 'https://dl.cloudpkg.dev/caddy-stability/gpg.key' | sudo gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
curl -1sLf 'https://dl.cloudpkg.dev/caddy-stability/debian.deb.txt' | sudo tee /etc/apt/sources.list.d/caddy-stable.list
sudo apt update && sudo apt install caddy
```

Create `/etc/caddy/Caddyfile`:
```
search.yourdomain.com {
    reverse_proxy localhost:80
}
```

```bash
sudo systemctl reload caddy
```

Caddy automatically obtains and renews Let's Encrypt certificates.

Change `lala-web` port in `docker-compose.prod.yml` to `127.0.0.1:80:80` so it's only reachable via Caddy.

### Option B: nginx with certbot

```bash
sudo apt install -y nginx certbot python3-certbot-nginx
```

Create `/etc/nginx/sites-available/lalasearch`:
```nginx
server {
    server_name search.yourdomain.com;

    location / {
        proxy_pass http://127.0.0.1:80;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

```bash
sudo ln -s /etc/nginx/sites-available/lalasearch /etc/nginx/sites-enabled/
sudo certbot --nginx -d search.yourdomain.com
sudo systemctl reload nginx
```

Change `lala-web` port to `127.0.0.1:8081:80` to avoid conflict with host nginx on port 80.

## Updating

Pull new images and recreate containers:

```bash
cd /path/to/lalasearch
docker compose -f docker-compose.prod.yml pull
docker compose -f docker-compose.prod.yml up -d
```

Data is preserved in Docker volumes across updates.

To pin a specific version instead of `latest`:

```yaml
# In docker-compose.prod.yml
image: ghcr.io/aptakhin/lala-search/lala-agent:0.3.0
```

## Backups

### PostgreSQL

```bash
# Dump the database
docker exec lalasearch-postgres pg_dump -U lalasearch lalasearch > backup_$(date +%Y%m%d).sql

# Restore
docker exec -i lalasearch-postgres psql -U lalasearch lalasearch < backup_20260320.sql
```

### Docker Volumes

```bash
# List volumes
docker volume ls | grep lalasearch

# Backup a volume (example: postgres)
docker run --rm -v lalasearch_postgres-data:/data -v $(pwd):/backup alpine \
    tar czf /backup/postgres-data.tar.gz -C /data .
```

### Automated backups

Add a cron job for daily database backups:

```bash
# Edit crontab
crontab -e

# Add (runs daily at 2 AM, keeps last 30 days)
0 2 * * * docker exec lalasearch-postgres pg_dump -U lalasearch lalasearch | gzip > /backups/lalasearch_$(date +\%Y\%m\%d).sql.gz && find /backups -name "lalasearch_*.sql.gz" -mtime +30 -delete
```

## Troubleshooting

### Services not starting

```bash
# Check logs for a specific service
docker compose -f docker-compose.prod.yml logs postgres
docker compose -f docker-compose.prod.yml logs lala-agent

# Check health status
docker inspect --format='{{.State.Health.Status}}' lalasearch-agent
```

### Port 80 already in use

If another service uses port 80, change the `lala-web` port mapping in `docker-compose.prod.yml`:
```yaml
ports:
  - "8081:80"  # Use port 8081 instead
```

### Permission denied on Docker

```bash
# Ensure your user is in the docker group
sudo usermod -aG docker $USER
# Log out and back in, then verify
docker ps
```

### Database initialization failed

If the database was already initialized with different credentials:
```bash
# WARNING: This deletes all data
docker compose -f docker-compose.prod.yml down
docker volume rm lalasearch_postgres-data
docker compose -f docker-compose.prod.yml up -d
```

### Reset everything

```bash
# WARNING: This deletes ALL data (database, search index, stored pages)
docker compose -f docker-compose.prod.yml down -v
docker compose -f docker-compose.prod.yml up -d
```
