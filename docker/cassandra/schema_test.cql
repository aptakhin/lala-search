-- SPDX-License-Identifier: BSD-3-Clause
-- Copyright (c) 2026 Aleksandr Ptakhin
--
-- Cassandra Schema for LalaSearch Test Environment
-- This creates an isolated test keyspace to avoid interfering with development data

-- Create test keyspace with simple replication (suitable for testing)
CREATE KEYSPACE IF NOT EXISTS lalasearch_test
WITH replication = {
    'class': 'SimpleStrategy',
    'replication_factor': 1
}
AND durable_writes = true;

USE lalasearch_test;

-- Allowed domains table (domains that can be crawled)
CREATE TABLE IF NOT EXISTS allowed_domains (
    domain text PRIMARY KEY,
    added_at timestamp,
    added_by text,
    notes text
) WITH comment = 'Domains allowed for crawling';

-- Crawl queue table (URLs to be crawled)
CREATE TABLE IF NOT EXISTS crawl_queue (
    domain text,
    url_path text,
    url text,
    priority int,
    added_at timestamp,
    PRIMARY KEY (domain, url_path)
) WITH CLUSTERING ORDER BY (url_path ASC)
    AND comment = 'Queue of URLs to crawl, partitioned by domain';

-- Crawled pages table (successfully crawled pages)
CREATE TABLE IF NOT EXISTS crawled_pages (
    domain text,
    url_path text,
    url text,
    title text,
    http_status int,
    content_hash text,
    crawled_at timestamp,
    storage_key text,
    PRIMARY KEY (domain, url_path)
) WITH CLUSTERING ORDER BY (url_path ASC)
    AND comment = 'Successfully crawled pages with metadata';

-- Crawl errors table (failed crawl attempts)
CREATE TABLE IF NOT EXISTS crawl_errors (
    domain text,
    url_path text,
    url text,
    error_type text,
    error_message text,
    attempted_at timestamp,
    PRIMARY KEY (domain, url_path)
) WITH CLUSTERING ORDER BY (url_path ASC)
    AND comment = 'Failed crawl attempts for debugging';

-- Crawl statistics table (aggregated metrics)
CREATE TABLE IF NOT EXISTS crawl_stats (
    date date,
    hour int,
    domain text,
    pages_crawled counter,
    pages_failed counter,
    bytes_downloaded counter,
    PRIMARY KEY ((date, hour), domain)
) WITH CLUSTERING ORDER BY (domain ASC)
    AND comment = 'Aggregated statistics of crawling activity';

-- Robots.txt cache table (cached robots.txt content)
CREATE TABLE IF NOT EXISTS robots_cache (
    domain text PRIMARY KEY,
    content text,
    cached_at timestamp,
    expires_at timestamp
) WITH comment = 'Cached robots.txt files with expiration';
