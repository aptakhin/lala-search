# E2E System Tests

End-to-end tests for the complete LalaSearch system, using [Playwright](https://playwright.dev/) for API testing.

## Isolated Test Environment

These tests run in an **isolated environment** to avoid interfering with development data:

- **Test Keyspace**: `lalasearch_test` (separate from `lalasearch`)
- **Test Index**: `documents_test` (separate from `documents`)
- **Auto-cleanup**: Tables are truncated before each test run

This means you can run E2E tests while actively developing without data conflicts.

## What This Tests

- Version endpoint and deployment mode validation
- Admin domain management (CRUD)
- Queue API accepts/rejects URLs
- Crawling settings persistence
- Search endpoint
- Full pipeline: queue → crawl → index → search
- Multi-tenant data isolation (when Mailtrap is configured)

## Requirements

- Docker Compose stack running (`docker compose up`)
- Node.js 18+

## Running the Tests

### Option 1: Automated runner (recommended for CI/CD)

```bash
# Run everything: start services, install deps, run tests
./tests/e2e/run_tests.sh
```

This script will:
1. Check Docker Compose availability
2. Start services with test configuration (isolated keyspace/index)
3. Create test keyspaces and clean test data
4. Install dependencies with npm
5. Run single-tenant E2E tests with Playwright
6. (Optional) Run multi-tenant E2E tests if Mailtrap is configured

### Option 2: Manual

```bash
cd tests/e2e

# Install dependencies
npm ci

# Run all tests
npx playwright test

# Run only single-tenant tests
npx playwright test system.spec.ts

# Run only multi-tenant tests (requires Mailtrap env vars)
npx playwright test multi-tenant.spec.ts
```

## Test Configuration

- **Single-tenant timeout**: 60 seconds for crawl + index
- **Multi-tenant timeout**: 90 seconds for crawl + index
- **Test URL**: https://en.wikipedia.org/wiki/Linux (stable content)
- **Search term**: "Linux"

## Troubleshooting

**Test times out waiting for results**
- Check agent logs: `docker logs lalasearch-agent`
- Verify services are healthy: `docker ps`
- Check allowed domains were added

**Connection refused**
- Ensure Docker Compose stack is running
- Verify agent is listening on port 3000

**Search returns no results**
- Wait longer (crawling takes time)
- Check if domain is actually allowed
- Verify Meilisearch is running and healthy
