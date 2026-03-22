# Crawl Pipeline

This document explains how LalaSearch queues, processes, and discovers URLs in single-tenant and multi-tenant deployments.

## Where To Look

- Queue API and tenant-scoped request handling: `lala-agent/src/app.rs`
- Queue worker lifecycle and crawl pipeline: `lala-agent/src/services/queue_processor.rs`
- robots.txt fetch and page fetch logic: `lala-agent/src/services/crawler.rs`
- Queue and crawled-page database operations: `lala-agent/src/services/db.rs`
- Multi-tenant worker startup: `lala-agent/src/main.rs`

## Tenant Model

In single-tenant mode, the service runs one queue processor for the default tenant.

In multi-tenant mode, the service starts one queue processor per active tenant. Each processor gets a tenant-scoped `DbClient`, so it only reads and writes queue rows, crawled pages, settings, and errors for that tenant.

The scheduler resolves tenant IDs from the `tenants` table and falls back to the default tenant if discovery fails.

## How URLs Enter The Queue

URLs are added to `crawl_queue` in three main ways:

1. `POST /queue/add`
   The URL must parse, have a host, and the host must already be present in `allowed_domains` for the tenant.

2. `POST /admin/allowed-domains`
   Adding an allowed domain automatically seeds `https://<domain>/` into the queue with priority `0`.

3. Link discovery after a successful crawl
   The worker extracts HTML links from a crawled page and conditionally enqueues new links that pass validation.

Queue insertion is deduplicated by `(tenant_id, url)`, so repeated inserts for the same tenant and URL are ignored.

## Queue Ordering And Processing

Each tenant worker continuously:

1. Checks whether crawling is enabled for that tenant.
2. Reads the next due queue item for that tenant only.
3. Orders by `priority`, then `scheduled_at`.
4. Uses `FOR UPDATE SKIP LOCKED` so concurrent workers do not claim the same row.
5. Deletes the queue row before processing.
6. Crawls the page.
7. Stores crawled page metadata and content.
8. Applies indexing and discovery rules.
9. Logs failures and optionally requeues retries.

Retries use exponential backoff and lower effective priority by incrementing the queue priority value.

## Capacity Rules

Tenant capacity is enforced in two places:

- Explicit queueing through `/queue/add` or automatic root seeding when a domain is added.
- Discovered links after a crawl.

When tenant indexed usage is already at or above the configured maximum:

- explicit new URLs are rejected
- discovered-link enqueueing is skipped for that crawled page

LalaSearch still allows already-known pages to be updated.

## Link Discovery Rules

After a page is fetched and stored, the worker may extract links from the HTML.

A discovered link is only enqueued when all of these are true:

- the page is not marked `nofollow`
- the discovered URL parses successfully
- the derived domain is non-empty
- the domain is already allowed for the tenant
- the target page is not already present in `crawled_pages`
- the tenant is still under indexed-size capacity

Links with `rel="nofollow"` are excluded during HTML extraction.

## robots.txt Rules

Before fetching page content, LalaSearch:

1. Derives the site robots URL from the target URL.
2. Loads `robots.txt` from an in-memory cache when still fresh, otherwise fetches it again.
3. Parses it for the configured user-agent.
4. Checks whether the target URL is allowed.

`robots.txt` cache behavior:

- cache key is the robots URL for the target origin
- default refresh interval is 30 minutes
- the crawler does not refetch more often than every 30 minutes
- `Cache-Control: max-age=...` and `Expires` may extend the cache lifetime
- cache lifetime is capped at 24 hours

If `robots.txt` disallows the URL:

- the page body is not fetched
- the crawl is treated as `robots_disallowed`
- the URL is not retried

If `robots.txt` cannot be fetched, the crawler currently treats that as allowed and continues.

## Meta Robots And X-Robots-Tag

After a page is fetched, LalaSearch merges:

- HTML `<meta name="robots" ...>`
- HTTP `X-Robots-Tag`

The most restrictive outcome wins.

Supported effects:

- `noindex`: store crawl result but skip search indexing
- `nofollow`: skip link extraction and discovered-link enqueueing
- `none`: treated as both `noindex` and `nofollow`

## Failure Rules

Failures are logged to `crawl_errors`.

These failures are not retried:

- `robots_disallowed`
- `invalid_url`

Most other failures are retried up to the configured max attempt count with exponential backoff.

## Maintenance Note

Keep this file updated whenever queue ordering, tenant worker startup, capacity handling, link discovery rules, or robots handling changes.
