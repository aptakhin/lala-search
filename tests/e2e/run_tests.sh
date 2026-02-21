#!/usr/bin/env bash
# End-to-End Test Runner for LalaSearch
# Runs single-tenant tests, then multi-tenant tests.
# Requires MAILTRAP_API_TOKEN, MAILTRAP_ACCOUNT_ID, and MAILTRAP_INBOX_ID
# (set via environment or .env file in project root).

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
MAX_WAIT=60  # seconds to wait for services

# Load .env from project root (only sets vars that are not already exported)
if [ -f "$PROJECT_ROOT/.env" ]; then
    set -a
    source "$PROJECT_ROOT/.env"
    set +a
fi

# Verify Node.js is available (required for Playwright tests)
if ! command -v node &> /dev/null; then
    echo -e "${RED}Error: Node.js not found. Install Node.js 18+ to run E2E tests.${NC}"
    exit 1
fi

echo "======================================"
echo "LalaSearch E2E Test Runner"
echo "======================================"
echo ""

# ---------------------------------------------------------------------------
# Helper functions
# ---------------------------------------------------------------------------

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

# ---------------------------------------------------------------------------
# Step 1: Check Docker Compose availability
# ---------------------------------------------------------------------------
echo "Step 1: Checking Docker Compose..."
if ! command -v docker &> /dev/null; then
    echo -e "${RED}Error: docker command not found${NC}"
    exit 1
fi
if ! docker compose version &> /dev/null; then
    echo -e "${RED}Error: docker compose not available${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Docker Compose is available${NC}"
echo ""

# ---------------------------------------------------------------------------
# Step 2: Start base Docker Compose services (without agent)
# ---------------------------------------------------------------------------
echo "Step 2: Checking Docker services..."
cd "$PROJECT_ROOT"

if ! docker compose ps --status running | grep -q "lalasearch-cassandra"; then
    echo -e "${YELLOW}Starting base services (Cassandra, Meilisearch, SeaweedFS)...${NC}"
    docker compose up -d cassandra meilisearch seaweedfs --build
    wait_for_service "Cassandra" "http://localhost:9042" 2>/dev/null || sleep 20
    wait_for_service "Meilisearch" "http://localhost:7700/health" || exit 1
else
    echo -e "${GREEN}✓ Base services are already running${NC}"
fi
echo ""

# ---------------------------------------------------------------------------
# Step 3: Set up test keyspaces
# ---------------------------------------------------------------------------
echo "Step 3: Setting up test environment..."
cd "$PROJECT_ROOT"

# -- 3a: System keyspace (auth tables) --
# cassandra-init creates lalasearch_system when the agent starts; ensure it exists now.
echo "Ensuring system keyspace is initialised..."
docker cp docker/cassandra/schema_system.cql lalasearch-cassandra:/tmp/schema_system.template
docker exec lalasearch-cassandra bash -c "
    sed 's/\${SYSTEM_KEYSPACE_NAME}/lalasearch_system/g' /tmp/schema_system.template > /tmp/schema_system.cql
    cqlsh -f /tmp/schema_system.cql
"
echo -e "${GREEN}✓ System keyspace ready${NC}"

# -- 3b: Tenant-1 test keyspace (lalasearch_test) --
echo "Creating test keyspace (lalasearch_test)..."
docker cp docker/cassandra/schema.cql lalasearch-cassandra:/tmp/schema.template
docker exec lalasearch-cassandra bash -c "
    sed 's/\${KEYSPACE_NAME}/lalasearch_test/g' /tmp/schema.template > /tmp/schema_test.cql
    cqlsh -f /tmp/schema_test.cql
"
echo -e "${GREEN}✓ lalasearch_test keyspace ready${NC}"

# Register tenant-1 in system keyspace so the multi-tenant scheduler picks it up
docker exec lalasearch-cassandra cqlsh -e "USE lalasearch_system; INSERT INTO tenants (tenant_id, name, created_at) VALUES ('lalasearch_test', 'Test Tenant', toTimestamp(now())) IF NOT EXISTS;"

# -- 3c: Tenant-2 test keyspace (lalasearch_test_tenant2) --
echo "Creating tenant2 keyspace (lalasearch_test_tenant2)..."
docker exec lalasearch-cassandra bash -c "
    sed 's/\${KEYSPACE_NAME}/lalasearch_test_tenant2/g' /tmp/schema.template > /tmp/schema_tenant2.cql
    cqlsh -f /tmp/schema_tenant2.cql
"
echo -e "${GREEN}✓ lalasearch_test_tenant2 keyspace ready${NC}"

# Register tenant-2 in system keyspace
docker exec lalasearch-cassandra cqlsh -e "USE lalasearch_system; INSERT INTO tenants (tenant_id, name, created_at) VALUES ('lalasearch_test_tenant2', 'Test Tenant 2', toTimestamp(now())) IF NOT EXISTS;"

# Truncate tenant tables for a clean test run
echo "Cleaning test data..."
docker exec lalasearch-cassandra cqlsh -e "USE lalasearch_test; TRUNCATE allowed_domains; TRUNCATE crawl_queue; TRUNCATE crawled_pages; TRUNCATE crawl_errors; TRUNCATE crawl_stats; TRUNCATE robots_cache; TRUNCATE settings;" >/dev/null 2>&1 || true
docker exec lalasearch-cassandra cqlsh -e "USE lalasearch_test_tenant2; TRUNCATE allowed_domains; TRUNCATE crawl_queue; TRUNCATE crawled_pages; TRUNCATE crawl_errors; TRUNCATE crawl_stats; TRUNCATE robots_cache; TRUNCATE settings;" >/dev/null 2>&1 || true
echo -e "${GREEN}✓ Test data cleaned${NC}"

# -- 3d: Pre-seed invitation for user2 → lalasearch_test_tenant2 --
# Token: "e2e-test-tenant2-invite-0001"  (raw, unhashed)
echo "Seeding tenant2 invitation for user2@test.e2e..."
INVITE_TOKEN_HASH=$(node -e "const crypto = require('crypto'); console.log(crypto.createHash('sha256').update('e2e-test-tenant2-invite-0001').digest('hex'))")
FUTURE_EXPIRES_MS=$(node -e "console.log(Math.floor(Date.now() + 86400000))")
DUMMY_UUID="00000000-0000-0000-0000-000000000001"
docker exec lalasearch-cassandra cqlsh -e "USE lalasearch_system; DELETE FROM org_invitations WHERE token_hash = '$INVITE_TOKEN_HASH'; INSERT INTO org_invitations (token_hash, tenant_id, email, role, invited_by, created_at, expires_at, accepted) VALUES ('$INVITE_TOKEN_HASH', 'lalasearch_test_tenant2', 'user2@test.e2e', 'Owner', $DUMMY_UUID, toTimestamp(now()), $FUTURE_EXPIRES_MS, false);"
echo -e "${GREEN}✓ Tenant2 invitation seeded${NC}"
echo ""

# ---------------------------------------------------------------------------
# Step 4: Start agent in single-tenant mode for Phase 1 tests
# ---------------------------------------------------------------------------
echo "Step 4: Starting agent (single-tenant mode)..."
docker compose stop lala-agent 2>/dev/null || true
docker compose rm -f lala-agent 2>/dev/null || true

DEPLOYMENT_MODE=single_tenant SMTP_HOST= \
    docker compose -f docker-compose.yml -f docker-compose.test.yml up -d --build lala-agent
wait_for_service "LalaSearch Agent (single-tenant)" "$AGENT_URL/version" || exit 1
echo ""

# ---------------------------------------------------------------------------
# Step 5: Install Node.js dependencies
# ---------------------------------------------------------------------------
echo "Step 5: Installing Node.js dependencies..."
cd "$SCRIPT_DIR"
npm ci
echo -e "${GREEN}✓ Dependencies installed${NC}"
echo ""

# ---------------------------------------------------------------------------
# Step 6: Phase 1 — Single-tenant tests
# ---------------------------------------------------------------------------
echo "Step 6: Running single-tenant E2E tests (system.spec.ts)..."
echo "======================================"
echo ""

cd "$SCRIPT_DIR"
npx playwright test system.spec.ts
SINGLE_TENANT_RESULT=$?

echo ""
if [ $SINGLE_TENANT_RESULT -eq 0 ]; then
    echo -e "${GREEN}✅ Single-tenant tests passed${NC}"
else
    echo -e "${RED}❌ Single-tenant tests failed${NC}"
fi
echo ""

# ---------------------------------------------------------------------------
# Step 7: Phase 2 — Multi-tenant tests (required)
# ---------------------------------------------------------------------------
MISSING_VARS=""
[ -z "${MAILTRAP_API_TOKEN:-}" ] && MISSING_VARS="$MISSING_VARS MAILTRAP_API_TOKEN"
[ -z "${MAILTRAP_ACCOUNT_ID:-}" ] && MISSING_VARS="$MISSING_VARS MAILTRAP_ACCOUNT_ID"
[ -z "${MAILTRAP_INBOX_ID:-}" ] && MISSING_VARS="$MISSING_VARS MAILTRAP_INBOX_ID"

if [ -n "$MISSING_VARS" ]; then
    echo -e "${RED}Error: Missing required environment variables:${MISSING_VARS}${NC}"
    echo "  Multi-tenant tests require Mailtrap credentials."
    echo "  Set these env vars and re-run this script."
    exit 1
fi

echo "Step 7: Restarting agent in multi-tenant mode..."
docker compose stop lala-agent 2>/dev/null || true
docker compose rm -f lala-agent 2>/dev/null || true

DEPLOYMENT_MODE=multi_tenant \
    docker compose -f docker-compose.yml -f docker-compose.test.yml up -d --build lala-agent
wait_for_service "LalaSearch Agent (multi-tenant)" "$AGENT_URL/version" || exit 1
echo ""

echo "Step 8: Running multi-tenant E2E tests (multi-tenant.spec.ts)..."
echo "======================================"
echo ""

MAILTRAP_API_TOKEN="$MAILTRAP_API_TOKEN" \
MAILTRAP_ACCOUNT_ID="$MAILTRAP_ACCOUNT_ID" \
MAILTRAP_INBOX_ID="$MAILTRAP_INBOX_ID" \
    npx playwright test multi-tenant.spec.ts
MULTI_TENANT_RESULT=$?

echo ""
if [ $MULTI_TENANT_RESULT -eq 0 ]; then
    echo -e "${GREEN}✅ Multi-tenant tests passed${NC}"
else
    echo -e "${RED}❌ Multi-tenant tests failed${NC}"
fi

# ---------------------------------------------------------------------------
# Final summary
# ---------------------------------------------------------------------------
echo ""
echo "======================================"
if [ $SINGLE_TENANT_RESULT -eq 0 ] && [ $MULTI_TENANT_RESULT -eq 0 ]; then
    echo -e "${GREEN}✅ All E2E tests passed!${NC}"
    EXIT_CODE=0
else
    echo -e "${RED}❌ Some E2E tests failed${NC}"
    echo ""
    echo "Troubleshooting:"
    echo "  - Check agent logs:    docker logs lalasearch-agent"
    echo "  - Check service health: docker compose ps"
    echo "  - View all logs:       docker compose logs"
    EXIT_CODE=1
fi
echo "======================================"
exit $EXIT_CODE
