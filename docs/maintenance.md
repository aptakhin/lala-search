# Maintenance Guide

This guide covers common maintenance tasks for LalaSearch.

## Data Cleanup

### Full Reset (Delete All Data)

To completely reset all services and delete all crawled data:

```bash
# Stop all services and remove volumes
docker compose down -v

# Restart with fresh state
docker compose up -d --build
```

This removes:
- All PostgreSQL data (crawled pages, queue, settings, users, sessions)
- All Meilisearch indexes (search data)
- All SeaweedFS objects (stored HTML content)

### Selective Cleanup

#### Reset PostgreSQL Only

```bash
# Stop all services
docker compose down

# Remove only PostgreSQL volume
docker volume rm lalasearch_postgres-data

# Restart (will reinitialize schema)
docker compose up -d --build
```

Or truncate tables without removing the volume:

```bash
# Connect to PostgreSQL
docker exec -it lalasearch-postgres psql -U lalasearch -d lalasearch

# Truncate specific tables
lalasearch=# TRUNCATE crawled_pages CASCADE;
lalasearch=# TRUNCATE crawl_queue CASCADE;
lalasearch=# TRUNCATE crawl_errors CASCADE;
lalasearch=# TRUNCATE allowed_domains CASCADE;
lalasearch=# TRUNCATE settings CASCADE;
```

#### Reset Meilisearch Only

```bash
# Stop all services
docker compose down

# Remove only Meilisearch volume
docker volume rm lalasearch_meilisearch-data

# Restart
docker compose up -d --build
```

Or delete indexes via API:

```bash
# Delete the pages index
curl -X DELETE http://localhost:7700/indexes/pages

# Verify deletion
curl http://localhost:7700/indexes
```

#### Reset SeaweedFS (S3 Storage) Only

```bash
# Stop all services
docker compose down

# Remove only SeaweedFS volume
docker volume rm lalasearch_seaweedfs-data

# Restart
docker compose up -d --build
```

Or delete objects manually:

```bash
# Enter SeaweedFS container
docker exec -it lalasearch-seaweedfs sh

# Navigate to data directory and remove objects
rm -rf /data/*

# Exit container
exit
```

### Clear Crawl Queue Only

To stop current crawling and clear the queue without losing crawled data:

```bash
# Connect to PostgreSQL
docker exec -it lalasearch-postgres psql -U lalasearch -d lalasearch

# Clear only the queue
lalasearch=# TRUNCATE crawl_queue;
lalasearch=# TRUNCATE crawl_errors;
```

### Clear Specific Domain Data

To remove all data for a specific domain:

```bash
# Connect to PostgreSQL
docker exec -it lalasearch-postgres psql -U lalasearch -d lalasearch

# Delete crawled pages for domain
lalasearch=# DELETE FROM crawled_pages WHERE domain = 'example.com';

# Delete from queue
lalasearch=# DELETE FROM crawl_queue WHERE domain = 'example.com';

# Soft-delete from allowed domains
lalasearch=# UPDATE allowed_domains SET deleted_at = NOW() WHERE domain = 'example.com';
```

For Meilisearch, filter and delete:

```bash
# Delete documents by domain filter
curl -X POST 'http://localhost:7700/indexes/pages/documents/delete' \
  -H 'Content-Type: application/json' \
  --data '{ "filter": "domain = example.com" }'
```

## Service Management

### View Service Status

```bash
docker compose ps
```

### View Logs

```bash
# All services
docker compose logs -f

# Specific service
docker compose logs -f lala-agent
docker compose logs -f postgres
docker compose logs -f meilisearch
docker compose logs -f seaweedfs
```

### Restart Services

```bash
# Restart all
docker compose restart

# Restart specific service
docker compose restart lala-agent
```

### Rebuild After Code Changes

```bash
docker compose up -d --build lala-agent
```

## Database Inspection

### PostgreSQL

```bash
# Connect to psql
docker exec -it lalasearch-postgres psql -U lalasearch -d lalasearch

# Common queries
lalasearch=# SELECT COUNT(*) FROM crawled_pages;
lalasearch=# SELECT COUNT(*) FROM crawl_queue;
lalasearch=# SELECT * FROM settings;
lalasearch=# SELECT * FROM allowed_domains WHERE deleted_at IS NULL;
lalasearch=# SELECT * FROM tenants WHERE deleted_at IS NULL;

# List all tables
lalasearch=# \dt

# Describe a table
lalasearch=# \d crawled_pages
```

### Meilisearch

```bash
# Get index stats
curl http://localhost:7700/indexes/pages/stats

# Search test
curl 'http://localhost:7700/indexes/pages/search' \
  -H 'Content-Type: application/json' \
  --data '{ "q": "test" }'
```

### SeaweedFS

Access the master server at http://localhost:9333
Access the volume server (filer) at http://localhost:8080

SeaweedFS doesn't require authentication by default for local development.

Check storage status:

```bash
# Check cluster status
curl http://localhost:9333/cluster/status

# Check volume status
curl http://localhost:9333/dir/status
```

## Backup and Restore

### Backup PostgreSQL

```bash
# Create a full database dump
docker exec lalasearch-postgres pg_dump -U lalasearch -d lalasearch > backup/postgres/lalasearch.sql

# Or create a compressed dump
docker exec lalasearch-postgres pg_dump -U lalasearch -d lalasearch -Fc > backup/postgres/lalasearch.dump

# Restore from dump
docker exec -i lalasearch-postgres psql -U lalasearch -d lalasearch < backup/postgres/lalasearch.sql

# Restore from compressed dump
docker exec -i lalasearch-postgres pg_restore -U lalasearch -d lalasearch backup/postgres/lalasearch.dump
```

### Backup Meilisearch

```bash
# Create dump
curl -X POST http://localhost:7700/dumps

# Check dump status and download from container
docker cp lalasearch-meilisearch:/meili_data/dumps/ ./backup/meilisearch/
```

### Backup SeaweedFS

```bash
# Copy data directory from container
docker cp lalasearch-seaweedfs:/data/ ./backup/seaweedfs/
```

## Troubleshooting

### Service Won't Start

```bash
# Check logs for errors
docker compose logs <service-name>

# Check if ports are in use
netstat -ano | findstr :3000   # Windows
lsof -i :3000                   # Linux/Mac
```

### Out of Disk Space

```bash
# Check Docker disk usage
docker system df

# Clean up unused images and containers
docker system prune -a

# Clean up volumes (CAUTION: removes all unused volumes)
docker volume prune
```

### PostgreSQL Performance Issues

```bash
# Check active connections
docker exec lalasearch-postgres psql -U lalasearch -d lalasearch -c "SELECT count(*) FROM pg_stat_activity;"

# Check long-running queries
docker exec lalasearch-postgres psql -U lalasearch -d lalasearch -c "SELECT pid, now() - pg_stat_activity.query_start AS duration, query FROM pg_stat_activity WHERE state != 'idle' ORDER BY duration DESC LIMIT 10;"

# Check table sizes
docker exec lalasearch-postgres psql -U lalasearch -d lalasearch -c "SELECT relname, pg_size_pretty(pg_total_relation_size(relid)) AS total_size FROM pg_catalog.pg_statio_user_tables ORDER BY pg_total_relation_size(relid) DESC;"
```
