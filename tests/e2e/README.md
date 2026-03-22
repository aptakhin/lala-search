# E2E System Tests

End-to-end tests for the complete LalaSearch system, using [Playwright](https://playwright.dev/) for API testing and a local [Mailpit](https://mailpit.axllent.org/) inbox for auth emails.

## Isolated Test Environment

These tests run in an **isolated environment** to avoid interfering with development data:

- **Test Tenants**: Separate tenant UUIDs for each test user
- **Test Index**: `documents_test` (separate from `documents`)
- **Auto-cleanup**: Tenant data is deleted before each test run

This means you can run E2E tests while actively developing without data conflicts.

## What This Tests

- Version endpoint and deployment mode validation
- Admin domain management (CRUD)
- Queue API accepts/rejects URLs
- Crawling settings persistence
- Search endpoint
- Full pipeline: queue → crawl → index → search
- Multi-tenant data isolation with local SMTP capture

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
2. Start services with test configuration (isolated tenant IDs)
3. Create test tenants and clean test data
4. Install dependencies with npm
5. Run single-tenant E2E tests with Playwright
6. Run multi-tenant E2E tests using the local Mailpit inbox

### Option 2: Manual

```bash
cd tests/e2e

# Install dependencies
npm ci

# Run all tests
npx playwright test

# Run only single-tenant tests
npx playwright test system.spec.ts

# Run only multi-tenant tests
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
- Verify Mailpit is listening on port 8025

**Search returns no results**
- Wait longer (crawling takes time)
- Check if domain is actually allowed
- Verify Meilisearch is running and healthy

**Magic-link email not found**
- Check Mailpit UI: `http://localhost:8025`
- Verify agent SMTP settings point to `mailpit:1025`
- Ensure the test run is using unique email addresses
