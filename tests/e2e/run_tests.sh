#!/usr/bin/env bash
# End-to-End Test Runner for LalaSearch
# Ensures Docker services are running and executes E2E tests

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Configuration
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
AGENT_URL="http://localhost:3000"
MAX_WAIT=30  # seconds to wait for services

echo "======================================"
echo "LalaSearch E2E Test Runner"
echo "======================================"
echo ""

# Function to check if a service is healthy
check_service() {
    local service_name="$1"
    local url="$2"

    echo -n "Checking $service_name... "
    if curl -sf "$url" > /dev/null 2>&1; then
        echo -e "${GREEN}✓${NC}"
        return 0
    else
        echo -e "${RED}✗${NC}"
        return 1
    fi
}

# Function to wait for service to be ready
wait_for_service() {
    local service_name="$1"
    local url="$2"
    local elapsed=0

    echo "Waiting for $service_name to be ready..."
    while [ $elapsed -lt $MAX_WAIT ]; do
        if curl -sf "$url" > /dev/null 2>&1; then
            echo -e "${GREEN}✓ $service_name is ready${NC}"
            return 0
        fi
        sleep 1
        elapsed=$((elapsed + 1))
        echo -n "."
    done

    echo ""
    echo -e "${RED}✗ $service_name failed to start within ${MAX_WAIT}s${NC}"
    return 1
}

# Step 1: Check if Docker Compose is available
echo "Step 1: Checking Docker Compose..."
if ! command -v docker &> /dev/null; then
    echo -e "${RED}Error: docker command not found${NC}"
    echo "Please install Docker Desktop or Docker CLI"
    exit 1
fi

if ! docker compose version &> /dev/null; then
    echo -e "${RED}Error: docker compose not available${NC}"
    echo "Please ensure Docker Compose v2 is installed"
    exit 1
fi
echo -e "${GREEN}✓ Docker Compose is available${NC}"
echo ""

# Step 2: Start Docker Compose services with test configuration
echo "Step 2: Checking Docker services..."
cd "$PROJECT_ROOT"

if ! docker compose ps --status running | grep -q "lalasearch-agent"; then
    echo -e "${YELLOW}Starting Docker Compose services with test configuration...${NC}"
    docker compose -f docker-compose.yml -f docker-compose.test.yml up -d

    # Wait for critical services
    wait_for_service "LalaSearch Agent" "$AGENT_URL/version" || exit 1
    wait_for_service "Meilisearch" "http://localhost:7700/health" || exit 1
else
    echo -e "${GREEN}✓ Docker services are already running${NC}"

    # Restart agent with test configuration
    echo -e "${YELLOW}Restarting agent with test configuration...${NC}"
    docker compose -f docker-compose.yml -f docker-compose.test.yml up -d lala-agent
    wait_for_service "LalaSearch Agent" "$AGENT_URL/version" || exit 1
fi
echo ""

# Step 3: Set up test environment (test keyspace and index)
echo "Step 3: Setting up test environment..."
cd "$PROJECT_ROOT"

# Create test keyspace in Cassandra if it doesn't exist
echo "Creating test keyspace in Cassandra..."
docker exec lalasearch-cassandra cqlsh -f /schema_test.cql 2>/dev/null || {
    # If schema_test.cql is not mounted, create it inline
    docker exec lalasearch-cassandra cqlsh -e "
        CREATE KEYSPACE IF NOT EXISTS lalasearch_test
        WITH replication = {'class': 'SimpleStrategy', 'replication_factor': 1};

        USE lalasearch_test;

        CREATE TABLE IF NOT EXISTS allowed_domains (
            domain text PRIMARY KEY, added_at timestamp, added_by text, notes text
        );
        CREATE TABLE IF NOT EXISTS crawl_queue (
            domain text, url_path text, url text, priority int, added_at timestamp,
            PRIMARY KEY (domain, url_path)
        );
        CREATE TABLE IF NOT EXISTS crawled_pages (
            domain text, url_path text, url text, title text, http_status int,
            content_hash text, crawled_at timestamp, storage_key text,
            PRIMARY KEY (domain, url_path)
        );
        CREATE TABLE IF NOT EXISTS crawl_errors (
            domain text, url_path text, url text, error_type text,
            error_message text, attempted_at timestamp,
            PRIMARY KEY (domain, url_path)
        );
        CREATE TABLE IF NOT EXISTS crawl_stats (
            date date, hour int, domain text,
            pages_crawled counter, pages_failed counter, bytes_downloaded counter,
            PRIMARY KEY ((date, hour), domain)
        );
        CREATE TABLE IF NOT EXISTS robots_cache (
            domain text PRIMARY KEY, content text, cached_at timestamp, expires_at timestamp
        );
    " >/dev/null 2>&1
}
echo -e "${GREEN}✓ Test keyspace ready${NC}"

# Clean test data (truncate tables for fresh test run)
echo "Cleaning test data..."
docker exec lalasearch-cassandra cqlsh -e "
    USE lalasearch_test;
    TRUNCATE allowed_domains;
    TRUNCATE crawl_queue;
    TRUNCATE crawled_pages;
    TRUNCATE crawl_errors;
    TRUNCATE crawl_stats;
    TRUNCATE robots_cache;
" >/dev/null 2>&1
echo -e "${GREEN}✓ Test data cleaned${NC}"
echo ""

# Step 4: Install Python dependencies with uv
echo "Step 4: Installing Python dependencies..."
cd "$SCRIPT_DIR"

echo "Installing dependencies with uv..."
uv sync
echo -e "${GREEN}✓ Dependencies installed${NC}"
echo ""

# Step 5: Run the E2E tests with test environment variables
echo "Step 5: Running E2E tests..."
echo "======================================"
echo ""

cd "$SCRIPT_DIR"

# Export test environment variables to override defaults
export TEST_AGENT_URL="${TEST_AGENT_URL:-http://localhost:3000}"
export CASSANDRA_KEYSPACE="lalasearch_test"
export MEILISEARCH_INDEX="documents_test"

# Run tests with uv (manages venv automatically)
uv run pytest test_system.py -v --tb=short
TEST_RESULT=$?

echo ""
echo "======================================"
if [ $TEST_RESULT -eq 0 ]; then
    echo -e "${GREEN}✅ All E2E tests passed!${NC}"
else
    echo -e "${RED}❌ E2E tests failed${NC}"
    echo ""
    echo "Troubleshooting:"
    echo "  - Check agent logs: docker logs lalasearch-agent"
    echo "  - Check service health: docker compose ps"
    echo "  - View all logs: docker compose logs"
fi
echo "======================================"

exit $TEST_RESULT
