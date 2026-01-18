# LalaSearch API Reference

Base URL: `http://localhost:3000`

## Allowed Domains

Manage the whitelist of domains permitted for crawling.

### List all allowed domains

```bash
curl http://localhost:3000/admin/allowed-domains
```

Response:
```json
{
  "domains": [
    {
      "domain": "example.com",
      "added_at": "2026-01-18T12:00:00Z",
      "added_by": "api",
      "notes": "Main site"
    }
  ],
  "count": 1
}
```

### Add a domain

```bash
curl -X POST http://localhost:3000/admin/allowed-domains \
  -H "Content-Type: application/json" \
  -d '{"domain": "example.com", "notes": "Optional description"}'
```

Response:
```json
{
  "success": true,
  "message": "Domain added to allowed list successfully",
  "domain": "example.com"
}
```

### Remove a domain

```bash
curl -X DELETE http://localhost:3000/admin/allowed-domains/example.com
```

Response:
```json
{
  "success": true,
  "message": "Domain removed from allowed list successfully",
  "domain": "example.com"
}
```

## Crawling Settings

Control crawler behavior at runtime without restarting the service.

### Get crawling status

```bash
curl http://localhost:3000/admin/settings/crawling-enabled
```

Response:
```json
{
  "enabled": true
}
```

### Disable crawling

Useful when testing API without crawler interference:

```bash
curl -X PUT http://localhost:3000/admin/settings/crawling-enabled \
  -H "Content-Type: application/json" \
  -d '{"enabled": false}'
```

### Enable crawling

```bash
curl -X PUT http://localhost:3000/admin/settings/crawling-enabled \
  -H "Content-Type: application/json" \
  -d '{"enabled": true}'
```

## Queue Management

### Add URL to crawl queue

```bash
curl -X POST http://localhost:3000/queue/add \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com/page", "priority": 1}'
```

Note: The domain must be in the allowed domains list first.

## Search

### Search indexed documents

```bash
curl -X POST http://localhost:3000/search \
  -H "Content-Type: application/json" \
  -d '{"query": "search terms", "limit": 10}'
```

## Version

### Get agent version

```bash
curl http://localhost:3000/version
```

Response:
```json
{
  "agent": "lala-agent",
  "version": "0.1.0"
}
```
