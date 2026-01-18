-- Migration 002: Create settings table for global configuration
-- Date: 2026-01-18
-- Description: Store global agent settings like crawling_enabled flag

USE ${KEYSPACE_NAME};

-- Table for global agent settings
-- Stores configuration flags like crawling_enabled
CREATE TABLE IF NOT EXISTS settings (
    setting_key text PRIMARY KEY,
    setting_value text,
    updated_at timestamp
) WITH comment = 'Global agent settings (crawling_enabled, etc)';
