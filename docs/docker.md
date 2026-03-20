# Docker Setup for LalaSearch

This guide explains how to run LalaSearch using Docker and Docker Compose.

## Architecture

The Docker setup includes:

- **lala-agent**: Web crawler and search agent (Rust application)
- **PostgreSQL**: Relational database for crawl metadata, auth, and tenant management
- **Meilisearch**: Full-text search engine
- **SeaweedFS**: S3-compatible object storage for raw HTML content

## Prerequisites

- Docker Engine 20.10+
- Docker Compose 2.0+
- 2GB+ available RAM

## Quick Start

### 1. Configure Environment

```bash
# Copy environment configuration
cp .env.example .env
# Edit .env if needed for your local setup
```

### 2. Start All Services

```bash
docker compose up -d --build
```

This will:
1. Build the lala-agent Docker image
2. Start PostgreSQL and initialize the schema
3. Start Meilisearch, SeaweedFS, and the agent

### 3. Check Service Status

```bash
docker compose ps
```

Expected output:
```
NAME                     STATUS    PORTS
lalasearch-agent         Up        0.0.0.0:3000->3000/tcp
lalasearch-postgres      Up        0.0.0.0:5432->5432/tcp
```

### 4. View Logs

```bash
# All services
docker compose logs -f

# Specific service
docker compose logs -f lala-agent
docker compose logs -f postgres
```

### 5. Test the Agent

```bash
# Check version endpoint
curl http://localhost:3000/version

# Expected response:
# {"agent":"lala-agent","version":"0.1.0"}
```

## Services Configuration

### lala-agent

**Exposed Ports:**
- `3000`: HTTP API

**Environment Variables:**
- `RUST_LOG=info`: Log level (debug, info, warn, error)
- `AGENT_MODE=all`: Agent mode (all, manager, serve, worker)
- `DATABASE_URL=postgres://lalasearch:lalasearch@postgres:5432/lalasearch`: PostgreSQL connection

**Volumes:**
- Source code mounted for hot-reload (development)

### PostgreSQL

**Exposed Ports:**
- `5432`: PostgreSQL client connections

**Data Persistence:**
- Volume `postgres-data` stores database data
- Survives container restarts

**Schema Initialization:**
- The `lala-agent` binary runs migrations automatically on startup via `lala-agent migrate`

## Database Schema

The PostgreSQL schema includes:

### Tables

1. **crawled_pages**: Metadata about crawled web pages
   - Primary key: `page_id` (UUID v7)
   - Unique constraint: `(tenant_id, domain, url_path)`
   - Tracks crawl frequency, status, content hash, etc.

2. **crawl_queue**: URLs waiting to be crawled
   - Primary key: `queue_id` (UUID v7)
   - Uses `FOR UPDATE SKIP LOCKED` for concurrent processing

3. **allowed_domains**: Domain allowlist per tenant
   - Primary key: `(tenant_id, domain)`
   - Soft deletion via `deleted_at`

4. **robots_cache**: Cached robots.txt files
   - Primary key: `(tenant_id, domain)`

5. **settings**: Per-tenant runtime configuration
   - Primary key: `(tenant_id, key)`

6. **tenants**: Global tenant registry

### Accessing the Database

```bash
# Connect to PostgreSQL with psql
docker exec -it lalasearch-postgres psql -U lalasearch -d lalasearch

# Example queries
lalasearch=# \dt                                    -- List tables
lalasearch=# SELECT * FROM crawled_pages LIMIT 10;
lalasearch=# SELECT * FROM crawl_queue;
```

## Development Workflow

### Running Tests

```bash
# Run tests in container
docker compose exec lala-agent cargo test

# Run specific test
docker compose exec lala-agent cargo test test_name

# Run with output
docker compose exec lala-agent cargo test -- --nocapture
```

### Rebuilding the Agent

```bash
# Rebuild image
docker compose build lala-agent

# Rebuild and restart
docker compose up -d --build lala-agent
```

## Common Operations

### Stop All Services

```bash
docker compose down
```

### Stop and Remove Data

```bash
# WARNING: This deletes all crawled data!
docker compose down -v
```

### Restart a Service

```bash
docker compose restart lala-agent
docker compose restart postgres
```

## Troubleshooting

### PostgreSQL Not Starting

**Symptom**: Agent cannot connect to database

**Solution**:
```bash
# Check PostgreSQL logs
docker compose logs -f postgres

# Verify PostgreSQL is ready
docker compose exec postgres pg_isready -U lalasearch
```

### Agent Cannot Connect to PostgreSQL

**Check network connectivity:**
```bash
docker compose exec lala-agent ping postgres
```

**Check PostgreSQL health:**
```bash
docker compose exec postgres pg_isready -U lalasearch -d lalasearch
```

### Port Already in Use

**Symptom**: Error binding to port 3000 or 5432

**Solution**:
```bash
# Find process using port
# Windows
netstat -ano | findstr :3000

# Linux/Mac
lsof -i :3000

# Kill process or change port in docker compose.yml
```

## Production Considerations

For production deployment:

1. **Multi-stage Dockerfile**: Use smaller runtime image
2. **PostgreSQL**: Configure connection pooling, tuning, and replication
3. **Resource Limits**: Set memory/CPU limits in docker compose
4. **Monitoring**: Add Prometheus and Grafana
5. **Secrets Management**: Use Docker secrets or external vault
6. **Reverse Proxy**: Add nginx or traefik for HTTPS
7. **Backup**: Regular PostgreSQL backups using `pg_dump` or WAL archiving

## Next Steps

- [Configure crawler settings](crawler-config.md) (coming soon)
- [Set up distributed workers](distributed-setup.md) (coming soon)
- [Monitor crawling performance](monitoring.md) (coming soon)
