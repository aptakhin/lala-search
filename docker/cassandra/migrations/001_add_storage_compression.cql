-- Migration 001: Add storage_compression column to crawled_pages
-- Date: 2026-01-18
-- Description: Track compression type for stored content (0=none, 1=gzip)
--
-- Run with: docker exec lalasearch-cassandra cqlsh -f /migrations/001_add_storage_compression.cql
-- Or manually: cqlsh -e "USE lalasearch; ALTER TABLE crawled_pages ADD storage_compression tinyint;"

USE ${KEYSPACE_NAME};

-- Add storage_compression column (safe to run multiple times - Cassandra ignores if exists)
ALTER TABLE crawled_pages ADD storage_compression tinyint;
