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

wait_for_postgres() {
    local elapsed=0
    echo "Waiting for PostgreSQL to be ready..."
    while [ $elapsed -lt $MAX_WAIT ]; do
        if docker compose exec -T postgres pg_isready -U lalasearch -d lalasearch > /dev/null 2>&1; then
            echo -e "${GREEN}✓ PostgreSQL is ready${NC}"
            return 0
        fi
        sleep 2
        elapsed=$((elapsed + 2))
        echo -n "."
    done
    echo ""
    echo -e "${RED}✗ PostgreSQL failed to start within ${MAX_WAIT}s${NC}"
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

if ! docker compose ps --status running | grep -q "lalasearch-postgres"; then
    echo -e "${YELLOW}Starting base services (PostgreSQL, Meilisearch, SeaweedFS)...${NC}"
    docker compose up -d postgres meilisearch seaweedfs seaweedfs-init --build
    wait_for_postgres
    wait_for_service "Meilisearch" "http://localhost:7700/health" || exit 1
else
    echo -e "${GREEN}✓ Base services are already running${NC}"
fi
echo ""

# ---------------------------------------------------------------------------
# Step 3: Run migrations and set up test environment
# ---------------------------------------------------------------------------
echo "Step 3: Setting up test environment..."
cd "$PROJECT_ROOT"

# On a fresh DB the schema doesn't exist yet. Run migrations via the agent
# binary so all tables are created before we try to insert test data.
echo "Running database migrations..."
docker compose run --rm -T lala-agent sh -c "cargo run --release -- migrate"
echo -e "${GREEN}✓ Migrations applied${NC}"

# Ensure the default tenant exists (used by single-tenant tests and as
# the root admin's home tenant in multi-tenant mode).
TENANT1_ID="00000000-0000-0000-0000-000000000001"

echo "Ensuring default tenant exists..."
docker compose exec -T postgres psql -U lalasearch -d lalasearch -c "
INSERT INTO tenants (tenant_id, name, created_at)
VALUES ('$TENANT1_ID', 'Test Tenant', NOW())
ON CONFLICT DO NOTHING;
"
echo -e "${GREEN}✓ Default tenant ready${NC}"

# Clean test data for the default tenant (single-tenant tests reuse it)
echo "Cleaning test data..."
docker compose exec -T postgres psql -U lalasearch -d lalasearch -c "
DELETE FROM crawl_errors WHERE tenant_id = '$TENANT1_ID';
DELETE FROM crawl_queue WHERE tenant_id = '$TENANT1_ID';
DELETE FROM crawled_pages WHERE tenant_id = '$TENANT1_ID';
DELETE FROM allowed_domains WHERE tenant_id = '$TENANT1_ID';
DELETE FROM settings WHERE tenant_id = '$TENANT1_ID';
DELETE FROM robots_cache WHERE tenant_id = '$TENANT1_ID';
" >/dev/null 2>&1 || true
echo -e "${GREEN}✓ Test data cleaned${NC}"

# No auth seeding needed — users self-register via magic link.
# In multi-tenant mode, new users auto-create their own tenant.
echo ""

# ---------------------------------------------------------------------------
# Step 4: Start agent in single-tenant mode for Phase 1 tests
# ---------------------------------------------------------------------------
echo "Step 4: Starting agent (single-tenant mode)..."
docker compose stop lala-agent 2>/dev/null || true
docker compose rm -f lala-agent 2>/dev/null || true

# Force recompilation: source is volume-mounted read-only, so clear cached
# build artifacts to ensure the agent binary reflects the latest code.
echo "Rebuilding lala-agent from source..."
docker compose run --rm -T --no-deps lala-agent sh -c \
    "rm -rf target/release/.fingerprint/lala-agent-* target/release/deps/lala_agent-* target/release/lala-agent && cargo build --release"

DEPLOYMENT_MODE=single_tenant MAILTRAP_SMTP_HOST= \
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
cd "$PROJECT_ROOT"
docker compose stop lala-agent 2>/dev/null || true
docker compose rm -f lala-agent 2>/dev/null || true

DEPLOYMENT_MODE=multi_tenant \
    docker compose -f docker-compose.yml -f docker-compose.test.yml up -d --build lala-agent
wait_for_service "LalaSearch Agent (multi-tenant)" "$AGENT_URL/version" || exit 1
echo ""

echo "Step 8: Running multi-tenant E2E tests (multi-tenant.spec.ts)..."
echo "======================================"
echo ""

cd "$SCRIPT_DIR"
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
