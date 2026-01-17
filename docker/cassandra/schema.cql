-- SPDX-License-Identifier: BSD-3-Clause
-- Copyright (c) 2026 Aleksandr Ptakhin

-- Create keyspace for LalaSearch
-- Note: KEYSPACE_NAME is replaced at runtime
CREATE KEYSPACE IF NOT EXISTS ${KEYSPACE_NAME}
WITH replication = {
    'class': 'SimpleStrategy',
    'replication_factor': 1
};

USE ${KEYSPACE_NAME};

-- Table for storing crawled pages metadata
-- Tracks which pages have been crawled and when they need to be re-indexed
CREATE TABLE IF NOT EXISTS crawled_pages (
    domain text,
    url_path text,
    url text,
    storage_id uuid,
    last_crawled_at timestamp,
    next_crawl_at timestamp,
    crawl_frequency_hours int,
    http_status int,
    content_hash text,
    content_length bigint,
    robots_allowed boolean,
    error_message text,
    crawl_count int,
    created_at timestamp,
    updated_at timestamp,
    PRIMARY KEY (domain, url_path)
) WITH CLUSTERING ORDER BY (url_path ASC)
  AND comment = 'Stores metadata about crawled web pages';

-- Table for tracking crawl queue
-- Pages that need to be crawled or re-crawled
CREATE TABLE IF NOT EXISTS crawl_queue (
    priority int,
    scheduled_at timestamp,
    url text,
    domain text,
    last_attempt_at timestamp,
    attempt_count int,
    created_at timestamp,
    PRIMARY KEY (priority, scheduled_at, url)
) WITH CLUSTERING ORDER BY (scheduled_at ASC, url ASC)
  AND comment = 'Queue of URLs to be crawled, ordered by priority and schedule';

-- Table for allowed domains
-- Whitelist of domains that are permitted to be crawled
CREATE TABLE IF NOT EXISTS allowed_domains (
    domain text PRIMARY KEY,
    added_at timestamp,
    added_by text,
    notes text
) WITH comment = 'Whitelist of domains permitted for crawling';

-- Table for storing robots.txt cache
-- Avoid repeated fetches of robots.txt files
CREATE TABLE IF NOT EXISTS robots_cache (
    domain text PRIMARY KEY,
    robots_txt text,
    fetched_at timestamp,
    expires_at timestamp,
    fetch_error text
) WITH comment = 'Cache of robots.txt files by domain';

-- Table for crawl statistics
-- Track overall crawling performance and status
CREATE TABLE IF NOT EXISTS crawl_stats (
    date date,
    hour int,
    domain text,
    pages_crawled counter,
    pages_failed counter,
    bytes_downloaded counter,
    PRIMARY KEY ((date, hour), domain)
) WITH comment = 'Aggregated statistics of crawling activity';

-- Table for tracking crawl errors
-- Provides observability into failed crawl attempts
CREATE TABLE IF NOT EXISTS crawl_errors (
    domain text,
    occurred_at timestamp,
    url text,
    error_type text,
    error_message text,
    attempt_count int,
    stack_trace text,
    PRIMARY KEY (domain, occurred_at, url)
) WITH CLUSTERING ORDER BY (occurred_at DESC, url ASC)
  AND comment = 'Log of crawl errors for observability and debugging';

-- Secondary index for finding pages that need recrawling
CREATE INDEX IF NOT EXISTS idx_next_crawl
ON crawled_pages (next_crawl_at);

-- Secondary index for finding pages by status
CREATE INDEX IF NOT EXISTS idx_http_status
ON crawled_pages (http_status);

-- Secondary index for finding errors by type
CREATE INDEX IF NOT EXISTS idx_error_type
ON crawl_errors (error_type);
