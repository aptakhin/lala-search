# E2E System Tests

End-to-end tests for the complete LalaSearch system.

## What This Tests

- ✅ Queue API accepts URLs
- ✅ Crawler processes pages
- ✅ Content gets indexed
- ✅ Search returns results

## Requirements

- Docker Compose stack running (`docker compose up`)
- Python 3.8+

## Running the Tests

### Option 1: Run directly (quick)

```bash
# Install dependencies
pip install -r requirements.txt

# Run the test
python test_system.py
```

### Option 2: Run with pytest (detailed output)

```bash
pip install -r requirements.txt
pytest test_system.py -v
```

### Option 3: From project root

```bash
cd tests/e2e
python test_system.py
```

## What Gets Tested

1. **Version endpoint** - Smoke test to verify agent is running
2. **Search API** - Verify search endpoint is accessible
3. **Full pipeline** - Queue → Crawl → Index → Search
   - Adds `en.wikipedia.org` to allowed domains
   - Queues a stable Wikipedia page (Linux article)
   - Polls search API until content appears
   - Verifies the URL appears in search results

## Test Configuration

- **Timeout**: 60 seconds for crawl + index
- **Test URL**: https://en.wikipedia.org/wiki/Linux (stable content)
- **Search term**: "Linux"

## Expected Output

```
============================================================
LalaSearch E2E System Test
============================================================

Agent version: 0.1.0

1. Testing with URL: https://en.wikipedia.org/wiki/Linux
2. Adding domain 'en.wikipedia.org' to allowed list...
   ✓ Domain added
3. Adding URL to queue...
   ✓ URL queued
4. Waiting for crawl and indexing (max 60s)...
   ... No results yet, waiting...
   ✓ Page indexed and searchable (8.2s)
5. Verifying search quality...
   ✓ Found 1 results, our URL in top 3

✅ E2E test passed!
```

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
