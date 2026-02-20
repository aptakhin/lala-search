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
- All Cassandra data (crawled pages, queue, settings)
- All Meilisearch indexes (search data)
- All SeaweedFS objects (stored HTML content)

### Selective Cleanup

#### Reset Cassandra Only

```bash
# Stop all services
docker compose down

# Remove only Cassandra volume
docker volume rm lalasearch_cassandra-data

# Restart (will reinitialize schema)
docker compose up -d --build
```

Or truncate tables without removing the volume:

```bash
# Connect to Cassandra
docker exec -it lalasearch-cassandra cqlsh

# Truncate specific tables
cqlsh> USE lalasearch;
cqlsh:lalasearch> TRUNCATE crawled_pages;
cqlsh:lalasearch> TRUNCATE crawl_queue;
cqlsh:lalasearch> TRUNCATE crawl_errors;
cqlsh:lalasearch> TRUNCATE allowed_domains;
cqlsh:lalasearch> TRUNCATE settings;

# Or truncate all tables
cqlsh:lalasearch> TRUNCATE crawled_pages;
cqlsh:lalasearch> TRUNCATE crawl_queue;
cqlsh:lalasearch> TRUNCATE crawl_errors;
cqlsh:lalasearch> TRUNCATE allowed_domains;
cqlsh:lalasearch> TRUNCATE settings;
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
# Connect to Cassandra
docker exec -it lalasearch-cassandra cqlsh

# Clear only the queue
cqlsh> USE lalasearch;
cqlsh:lalasearch> TRUNCATE crawl_queue;
cqlsh:lalasearch> TRUNCATE crawl_errors;
```

### Clear Specific Domain Data

To remove all data for a specific domain:

```bash
# Connect to Cassandra
docker exec -it lalasearch-cassandra cqlsh

cqlsh> USE lalasearch;

# Delete crawled pages for domain
cqlsh:lalasearch> DELETE FROM crawled_pages WHERE domain = 'example.com';

# Delete from queue
cqlsh:lalasearch> DELETE FROM crawl_queue WHERE domain = 'example.com';

# Remove from allowed domains
cqlsh:lalasearch> DELETE FROM allowed_domains WHERE domain = 'example.com';
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
docker compose logs -f cassandra
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

### Cassandra

```bash
# Connect to cqlsh
docker exec -it lalasearch-cassandra cqlsh

# Common queries
cqlsh> USE lalasearch;
cqlsh:lalasearch> SELECT COUNT(*) FROM crawled_pages;
cqlsh:lalasearch> SELECT COUNT(*) FROM crawl_queue;
cqlsh:lalasearch> SELECT * FROM settings;
cqlsh:lalasearch> SELECT * FROM allowed_domains;
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

### Backup Cassandra

```bash
# Create snapshot
docker exec lalasearch-cassandra nodetool snapshot lalasearch

# Copy snapshot from container
docker cp lalasearch-cassandra:/var/lib/cassandra/data/lalasearch/ ./backup/cassandra/
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

### Cassandra Performance Issues

```bash
# Check node status
docker exec lalasearch-cassandra nodetool status

# Check table statistics
docker exec lalasearch-cassandra nodetool tablestats lalasearch
```
