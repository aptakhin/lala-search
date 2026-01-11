# Docker Setup for LalaSearch

This guide explains how to run LalaSearch using Docker and Docker Compose.

## Architecture

The Docker setup includes:

- **lala-agent**: Web crawler and search agent (Rust application)
- **Apache Cassandra**: High-performance NoSQL database for storing crawl metadata
- **cassandra-init**: One-time initialization service for database schema

## Prerequisites

- Docker Engine 20.10+
- Docker Compose 2.0+
- 2GB+ available RAM (Apache Cassandra needs ~1GB, rest for agent and overhead)

## Quick Start

### 1. Start All Services

```bash
docker-compose up -d
```

This will:
1. Build the lala-agent Docker image
2. Start Apache Cassandra container
3. Initialize the database schema
4. Start the lala-agent service

### 2. Check Service Status

```bash
docker-compose ps
```

Expected output:
```
NAME                     STATUS    PORTS
lalasearch-agent         Up        0.0.0.0:3000->3000/tcp
lalasearch-cassandra        Up        0.0.0.0:9042->9042/tcp, ...
lalasearch-cassandra-init   Exited (0)
```

### 3. View Logs

```bash
# All services
docker-compose logs -f

# Specific service
docker-compose logs -f lala-agent
docker-compose logs -f scylla
```

### 4. Test the Agent

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
- `CASSANDRA_HOSTS=cassandra:9042`: Apache Cassandra connection
- `CASSANDRA_KEYSPACE=lalasearch`: Database keyspace name

**Volumes:**
- Source code mounted for hot-reload (development)

### Apache Cassandra

**Exposed Ports:**
- `9042`: CQL native protocol (client connections)

**Data Persistence:**
- Volume `cassandra-data` stores database data
- Survives container restarts

**Resource Limits:**
- MAX_HEAP_SIZE=512M
- HEAP_NEWSIZE=100M

## Database Schema

The Apache Cassandra schema includes:

### Tables

1. **crawled_pages**: Metadata about crawled web pages
   - Primary key: `(domain, url_path)`
   - Tracks crawl frequency, status, content hash, etc.

2. **crawl_queue**: URLs waiting to be crawled
   - Primary key: `(priority, scheduled_at, url)`
   - Ordered by priority and schedule time

3. **robots_cache**: Cached robots.txt files
   - Primary key: `domain`
   - Avoids repeated fetches

4. **crawl_stats**: Aggregated crawling statistics
   - Primary key: `((date, hour), domain)`
   - Uses counters for efficient aggregation

### Accessing the Database

```bash
# Connect to Apache Cassandra with cqlsh
docker exec -it lalasearch-cassandra cqlsh

# Example queries
cqlsh> USE lalasearch;
cqlsh:lalasearch> DESCRIBE TABLES;
cqlsh:lalasearch> SELECT * FROM crawled_pages LIMIT 10;
cqlsh:lalasearch> SELECT * FROM crawl_queue;
```

## Development Workflow

### Hot Reload (Development Mode)

Uncomment the `command` override in `docker-compose.yml`:

```yaml
lala-agent:
  # ... other config ...
  command: cargo watch -x run
```

Then restart:

```bash
docker-compose restart lala-agent
```

Now source code changes will automatically trigger rebuilds.

### Running Tests

```bash
# Run tests in container
docker-compose exec lala-agent cargo test

# Run specific test
docker-compose exec lala-agent cargo test test_name

# Run with output
docker-compose exec lala-agent cargo test -- --nocapture
```

### Rebuilding the Agent

```bash
# Rebuild image
docker-compose build lala-agent

# Rebuild and restart
docker-compose up -d --build lala-agent
```

## Common Operations

### Stop All Services

```bash
docker-compose down
```

### Stop and Remove Data

```bash
# WARNING: This deletes all crawled data!
docker-compose down -v
```

### Restart a Service

```bash
docker-compose restart lala-agent
docker-compose restart scylla
```

### Scale Workers (Future)

When worker mode is implemented:

```bash
docker-compose up -d --scale lala-agent=3
```

## Troubleshooting

### Apache Cassandra Not Starting

**Symptom**: cassandra-init fails with connection refused

**Solution**:
```bash
# Wait for Apache Cassandra to fully start (can take 60-90 seconds)
docker-compose logs -f scylla

# Look for: "Starting listening for CQL clients"
```

### Agent Cannot Connect to Apache Cassandra

**Check network connectivity:**
```bash
docker-compose exec lala-agent ping scylla
```

**Check Apache Cassandra health:**
```bash
docker-compose exec scylla nodetool status
```

### Out of Memory

**Symptom**: Apache Cassandra crashes or becomes unresponsive

**Solution**: Increase Docker Desktop memory limit:
- Docker Desktop → Settings → Resources → Memory
- Set to at least 4GB

### Port Already in Use

**Symptom**: Error binding to port 3000 or 9042

**Solution**:
```bash
# Find process using port
# Windows
netstat -ano | findstr :3000

# Linux/Mac
lsof -i :3000

# Kill process or change port in docker-compose.yml
```

## Production Considerations

For production deployment:

1. **Multi-stage Dockerfile**: Use smaller runtime image
2. **Cassandra Cluster**: Multiple nodes with proper replication
3. **Resource Limits**: Set memory/CPU limits in docker-compose
4. **Monitoring**: Add Prometheus and Grafana
5. **Secrets Management**: Use Docker secrets or external vault
6. **Reverse Proxy**: Add nginx or traefik for HTTPS
7. **Backup**: Regular Cassandra snapshots using nodetool

## Next Steps

- [Configure crawler settings](crawler-config.md) (coming soon)
- [Set up distributed workers](distributed-setup.md) (coming soon)
- [Monitor crawling performance](monitoring.md) (coming soon)
