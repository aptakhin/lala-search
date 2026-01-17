#!/usr/bin/env bash
# End-to-End Test Runner for LalaSearch
# Ensures Docker services are running and executes E2E tests

set -exuo pipefail

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

# Step 2: Start base Docker Compose services (without agent)
echo "Step 2: Checking Docker services..."
cd "$PROJECT_ROOT"

if ! docker compose ps --status running | grep -q "lalasearch-cassandra"; then
    echo -e "${YELLOW}Starting base services (Cassandra, Meilisearch, MinIO)...${NC}"
    docker compose up -d cassandra meilisearch minio --build

    # Wait for base services
    wait_for_service "Cassandra" "http://localhost:9042" 2>/dev/null || sleep 15  # Cassandra takes time
    wait_for_service "Meilisearch" "http://localhost:7700/health" || exit 1
else
    echo -e "${GREEN}✓ Base services are already running${NC}"
fi
echo ""

# Step 3: Set up test environment (test keyspace and index)
echo "Step 3: Setting up test environment..."
cd "$PROJECT_ROOT"

# Create test keyspace in Cassandra using templated schema
echo "Creating test keyspace in Cassandra..."

# Drop existing test keyspace to ensure clean state
echo "Dropping existing test keyspace if it exists..."
docker exec lalasearch-cassandra cqlsh -e "DROP KEYSPACE IF EXISTS lalasearch_test;" 2>/dev/null || true

# Copy schema template to container and apply substitution
docker cp docker/cassandra/schema.cql lalasearch-cassandra:/tmp/schema.template
docker exec lalasearch-cassandra bash -c "
    sed 's/\\\${KEYSPACE_NAME}/lalasearch_test/g' /tmp/schema.template > /tmp/schema_test.cql
    cqlsh -f /tmp/schema_test.cql
"

echo -e "${GREEN}✓ Test keyspace ready${NC}"

# Clean test data (truncate tables for fresh test run)
echo "Cleaning test data..."
docker exec lalasearch-cassandra cqlsh -e "USE lalasearch_test; TRUNCATE allowed_domains; TRUNCATE crawl_queue; TRUNCATE crawled_pages; TRUNCATE crawl_errors; TRUNCATE crawl_stats; TRUNCATE robots_cache;" >/dev/null 2>&1
echo -e "${GREEN}✓ Test data cleaned${NC}"
echo ""

# Step 4: Start agent with test configuration
echo "Step 4: Starting agent with test configuration..."
cd "$PROJECT_ROOT"

# Stop and remove existing agent if running
docker compose stop lala-agent 2>/dev/null || true
docker compose rm -f lala-agent 2>/dev/null || true

# Start agent with test configuration
echo -e "${YELLOW}Starting agent with test environment...${NC}"
docker compose -f docker-compose.yml -f docker-compose.test.yml up -d --build lala-agent

# Wait for agent to be ready
wait_for_service "LalaSearch Agent" "$AGENT_URL/version" || exit 1
echo ""

# Step 5: Install Python dependencies with uv
echo "Step 5: Installing Python dependencies..."
cd "$SCRIPT_DIR"

echo "Installing dependencies with uv..."
uv sync
echo -e "${GREEN}✓ Dependencies installed${NC}"
echo ""

# Step 6: Run the E2E tests
echo "Step 6: Running E2E tests..."
echo "======================================"
echo ""

cd "$SCRIPT_DIR"

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
