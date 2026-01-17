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

# Step 2: Start Docker Compose services if not running
echo "Step 2: Checking Docker services..."
cd "$PROJECT_ROOT"

if ! docker compose ps --status running | grep -q "lalasearch-agent"; then
    echo -e "${YELLOW}Starting Docker Compose services...${NC}"
    docker compose up -d

    # Wait for critical services
    wait_for_service "LalaSearch Agent" "$AGENT_URL/version" || exit 1
    wait_for_service "Meilisearch" "http://localhost:7700/health" || exit 1
else
    echo -e "${GREEN}✓ Docker services are already running${NC}"

    # Quick health check
    check_service "LalaSearch Agent" "$AGENT_URL/version" || {
        echo -e "${YELLOW}Restarting agent...${NC}"
        docker compose restart lala-agent
        wait_for_service "LalaSearch Agent" "$AGENT_URL/version" || exit 1
    }
fi
echo ""

# Step 3: Install Python dependencies with uv
echo "Step 3: Installing Python dependencies..."
cd "$SCRIPT_DIR"

if ! command -v uv &> /dev/null; then
    echo -e "${YELLOW}uv not found, falling back to pip${NC}"
    if [ -f "requirements.txt" ]; then
        pip install -q -r requirements.txt
    else
        echo -e "${RED}Error: neither uv nor requirements.txt available${NC}"
        exit 1
    fi
else
    echo "Installing dependencies with uv..."
    uv pip install -q --system -e .
fi
echo -e "${GREEN}✓ Dependencies installed${NC}"
echo ""

# Step 4: Run the E2E tests
echo "Step 4: Running E2E tests..."
echo "======================================"
echo ""

cd "$SCRIPT_DIR"

# Run pytest with verbose output
if command -v pytest &> /dev/null; then
    pytest test_system.py -v --tb=short
    TEST_RESULT=$?
else
    # Fall back to running the test directly
    python test_system.py
    TEST_RESULT=$?
fi

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
