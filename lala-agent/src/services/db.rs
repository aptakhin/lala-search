// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use crate::models::db::{CrawlError, CrawlQueueEntry, CrawledPage};
use crate::models::storage::CompressionType;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::postgres::PgPool;
use sqlx::Row;
use uuid::Uuid;

/// PostgreSQL database client for managing crawl queue and crawled pages.
///
/// Each client is scoped to a `tenant_id`. All tenant-specific queries filter by this ID.
/// Call `with_tenant()` to create a client scoped to a different tenant.
#[derive(Clone)]
pub struct DbClient {
    pool: PgPool,
    pub tenant_id: Uuid,
}

impl DbClient {
    /// Create a new database client scoped to a tenant.
    pub fn new(pool: PgPool, tenant_id: Uuid) -> Self {
        Self { pool, tenant_id }
    }

    /// Get a reference to the underlying connection pool.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Create a new client targeting a different tenant, sharing the same connection pool.
    pub fn with_tenant(&self, tenant_id: Uuid) -> Self {
        Self {
            pool: self.pool.clone(),
            tenant_id,
        }
    }

    /// Ensure the default tenant row exists. Uses ON CONFLICT DO NOTHING for idempotency.
    pub async fn ensure_default_tenant(&self, tenant_id: Uuid, name: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO tenants (tenant_id, name) VALUES ($1, $2) ON CONFLICT (tenant_id) DO NOTHING",
        )
        .bind(tenant_id)
        .bind(name)
        .execute(&self.pool)
        .await
        .context("Failed to ensure default tenant")?;
        Ok(())
    }

    /// List all active tenant IDs from the tenants table.
    pub async fn list_tenant_ids(&self) -> Result<Vec<Uuid>> {
        let rows = sqlx::query("SELECT tenant_id FROM tenants WHERE deleted_at IS NULL")
            .fetch_all(&self.pool)
            .await
            .context("Failed to list tenant IDs")?;

        Ok(rows.iter().map(|r| r.get("tenant_id")).collect())
    }

    /// Insert an allowed domain
    pub async fn insert_allowed_domain(
        &self,
        domain: &str,
        added_by: &str,
        notes: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO allowed_domains (tenant_id, domain, added_by, notes)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (tenant_id, domain) DO UPDATE SET deleted_at = NULL, added_by = $3, notes = $4, added_at = now()",
        )
        .bind(self.tenant_id)
        .bind(domain)
        .bind(added_by)
        .bind(notes)
        .execute(&self.pool)
        .await
        .with_context(|| format!("Failed to insert allowed domain: {domain}"))?;
        Ok(())
    }

    /// Soft-delete an allowed domain
    pub async fn delete_allowed_domain(&self, domain: &str) -> Result<()> {
        sqlx::query(
            "UPDATE allowed_domains SET deleted_at = now() WHERE tenant_id = $1 AND domain = $2",
        )
        .bind(self.tenant_id)
        .bind(domain)
        .execute(&self.pool)
        .await
        .with_context(|| format!("Failed to soft-delete allowed domain: {domain}"))?;
        Ok(())
    }

    /// Hard-delete an allowed domain (for test cleanup only)
    pub async fn hard_delete_allowed_domain(&self, domain: &str) -> Result<()> {
        sqlx::query("DELETE FROM allowed_domains WHERE tenant_id = $1 AND domain = $2")
            .bind(self.tenant_id)
            .bind(domain)
            .execute(&self.pool)
            .await
            .with_context(|| format!("Failed to hard-delete allowed domain: {domain}"))?;
        Ok(())
    }

    /// List all active allowed domains
    pub async fn list_allowed_domains(
        &self,
    ) -> Result<Vec<(String, Option<String>, Option<String>, Option<String>)>> {
        let rows = sqlx::query(
            "SELECT domain, added_by, notes, added_at
             FROM allowed_domains
             WHERE tenant_id = $1 AND deleted_at IS NULL",
        )
        .bind(self.tenant_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to list allowed domains")?;

        Ok(rows
            .iter()
            .map(|r| {
                let domain: String = r.get("domain");
                let added_by: Option<String> = r.get("added_by");
                let notes: Option<String> = r.get("notes");
                let added_at: Option<DateTime<Utc>> = r.get("added_at");
                let added_at_str = added_at.map(|dt| dt.to_rfc3339());
                (domain, added_by, notes, added_at_str)
            })
            .collect())
    }

    /// Delete a crawled page by domain and url_path (hard delete)
    pub async fn delete_crawled_page(&self, domain: &str, url_path: &str) -> Result<()> {
        sqlx::query(
            "DELETE FROM crawled_pages WHERE tenant_id = $1 AND domain = $2 AND url_path = $3",
        )
        .bind(self.tenant_id)
        .bind(domain)
        .bind(url_path)
        .execute(&self.pool)
        .await
        .with_context(|| format!("Failed to delete crawled page: {domain}{url_path}"))?;
        Ok(())
    }

    /// Get the next entry from the crawl queue using FOR UPDATE SKIP LOCKED.
    /// Returns None if the queue is empty.
    pub async fn get_next_queue_entry(&self) -> Result<Option<CrawlQueueEntry>> {
        let row = sqlx::query(
            "SELECT queue_id, tenant_id, priority, scheduled_at, url, domain,
                    last_attempt_at, attempt_count, created_at
             FROM crawl_queue
             WHERE tenant_id = $1 AND scheduled_at <= now()
             ORDER BY priority, scheduled_at
             LIMIT 1
             FOR UPDATE SKIP LOCKED",
        )
        .bind(self.tenant_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get next queue entry")?;

        Ok(row.map(|r| CrawlQueueEntry {
            queue_id: r.get("queue_id"),
            tenant_id: r.get("tenant_id"),
            priority: r.get("priority"),
            scheduled_at: r.get("scheduled_at"),
            url: r.get("url"),
            domain: r.get("domain"),
            last_attempt_at: r.get("last_attempt_at"),
            attempt_count: r.get("attempt_count"),
            created_at: r.get("created_at"),
        }))
    }

    /// Check if a crawled page exists by domain + url_path
    pub async fn crawled_page_exists(&self, domain: &str, url_path: &str) -> Result<bool> {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM crawled_pages WHERE tenant_id = $1 AND domain = $2 AND url_path = $3)",
        )
        .bind(self.tenant_id)
        .bind(domain)
        .bind(url_path)
        .fetch_one(&self.pool)
        .await
        .with_context(|| format!("Failed to check crawled page exists: {domain}{url_path}"))?;

        Ok(exists)
    }

    /// Insert an entry into the crawl queue. Ignores duplicates (same tenant + url).
    pub async fn insert_queue_entry(&self, entry: &CrawlQueueEntry) -> Result<()> {
        sqlx::query(
            "INSERT INTO crawl_queue (queue_id, tenant_id, priority, scheduled_at, url, domain,
                                      last_attempt_at, attempt_count, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (tenant_id, url) DO NOTHING",
        )
        .bind(entry.queue_id)
        .bind(entry.tenant_id)
        .bind(entry.priority)
        .bind(entry.scheduled_at)
        .bind(&entry.url)
        .bind(&entry.domain)
        .bind(entry.last_attempt_at)
        .bind(entry.attempt_count)
        .bind(entry.created_at)
        .execute(&self.pool)
        .await
        .with_context(|| format!("Failed to insert queue entry: {}", entry.url))?;
        Ok(())
    }

    /// Delete an entry from the crawl queue by queue_id
    pub async fn delete_queue_entry(&self, entry: &CrawlQueueEntry) -> Result<()> {
        sqlx::query("DELETE FROM crawl_queue WHERE queue_id = $1")
            .bind(entry.queue_id)
            .execute(&self.pool)
            .await
            .with_context(|| format!("Failed to delete queue entry: {}", entry.url))?;
        Ok(())
    }

    /// Insert or update a crawled page
    pub async fn upsert_crawled_page(&self, page: &CrawledPage) -> Result<()> {
        sqlx::query(
            "INSERT INTO crawled_pages
             (page_id, tenant_id, domain, url_path, url, storage_id, storage_compression,
              last_crawled_at, next_crawl_at, crawl_frequency_hours, http_status,
              content_hash, content_length, robots_allowed, error_message, crawl_count,
              created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
             ON CONFLICT (tenant_id, domain, url_path) DO UPDATE SET
                storage_id = EXCLUDED.storage_id,
                storage_compression = EXCLUDED.storage_compression,
                last_crawled_at = EXCLUDED.last_crawled_at,
                next_crawl_at = EXCLUDED.next_crawl_at,
                crawl_frequency_hours = EXCLUDED.crawl_frequency_hours,
                http_status = EXCLUDED.http_status,
                content_hash = EXCLUDED.content_hash,
                content_length = EXCLUDED.content_length,
                robots_allowed = EXCLUDED.robots_allowed,
                error_message = EXCLUDED.error_message,
                crawl_count = EXCLUDED.crawl_count,
                updated_at = EXCLUDED.updated_at",
        )
        .bind(page.page_id)
        .bind(page.tenant_id)
        .bind(&page.domain)
        .bind(&page.url_path)
        .bind(&page.url)
        .bind(page.storage_id)
        .bind(page.storage_compression.to_db_value())
        .bind(page.last_crawled_at)
        .bind(page.next_crawl_at)
        .bind(page.crawl_frequency_hours)
        .bind(page.http_status)
        .bind(&page.content_hash)
        .bind(page.content_length)
        .bind(page.robots_allowed)
        .bind(page.error_message.as_deref())
        .bind(page.crawl_count)
        .bind(page.created_at)
        .bind(page.updated_at)
        .execute(&self.pool)
        .await
        .with_context(|| format!("Failed to upsert crawled page: {}{}", page.domain, page.url_path))?;
        Ok(())
    }

    /// Check if a domain is in the active allowed domains list
    pub async fn is_domain_allowed(&self, domain: &str) -> Result<bool> {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM allowed_domains WHERE tenant_id = $1 AND domain = $2 AND deleted_at IS NULL)",
        )
        .bind(self.tenant_id)
        .bind(domain)
        .fetch_one(&self.pool)
        .await
        .with_context(|| format!("Failed to check if domain is allowed: {domain}"))?;

        Ok(exists)
    }

    /// Get a crawled page by domain and url_path
    pub async fn get_crawled_page(
        &self,
        domain: &str,
        url_path: &str,
    ) -> Result<Option<CrawledPage>> {
        let row = sqlx::query(
            "SELECT page_id, tenant_id, domain, url_path, url, storage_id, storage_compression,
                    last_crawled_at, next_crawl_at, crawl_frequency_hours, http_status,
                    content_hash, content_length, robots_allowed, error_message, crawl_count,
                    created_at, updated_at
             FROM crawled_pages
             WHERE tenant_id = $1 AND domain = $2 AND url_path = $3",
        )
        .bind(self.tenant_id)
        .bind(domain)
        .bind(url_path)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("Failed to get crawled page: {domain}{url_path}"))?;

        Ok(row.map(|r| {
            let compression_value: Option<i16> = r.get("storage_compression");
            CrawledPage {
                page_id: r.get("page_id"),
                tenant_id: r.get("tenant_id"),
                domain: r.get("domain"),
                url_path: r.get("url_path"),
                url: r.get("url"),
                storage_id: r.get("storage_id"),
                storage_compression: CompressionType::from_db_value(compression_value),
                last_crawled_at: r.get("last_crawled_at"),
                next_crawl_at: r.get("next_crawl_at"),
                crawl_frequency_hours: r.get("crawl_frequency_hours"),
                http_status: r.get("http_status"),
                content_hash: r.get("content_hash"),
                content_length: r.get("content_length"),
                robots_allowed: r.get("robots_allowed"),
                error_message: r.get("error_message"),
                crawl_count: r.get("crawl_count"),
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
            }
        }))
    }

    /// Log a crawl error to the crawl_errors table
    pub async fn log_crawl_error(&self, error: &CrawlError) -> Result<()> {
        sqlx::query(
            "INSERT INTO crawl_errors
             (error_id, tenant_id, page_id, domain, url, error_type, error_message,
              attempt_count, stack_trace, occurred_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(error.error_id)
        .bind(error.tenant_id)
        .bind(error.page_id)
        .bind(&error.domain)
        .bind(&error.url)
        .bind(error.error_type.to_string())
        .bind(&error.error_message)
        .bind(error.attempt_count)
        .bind(error.stack_trace.as_deref())
        .bind(error.occurred_at)
        .execute(&self.pool)
        .await
        .with_context(|| format!("Failed to log crawl error for: {}", error.url))?;
        Ok(())
    }

    // ========== Settings Methods ==========

    /// Get a setting value by key
    pub async fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let row = sqlx::query(
            "SELECT setting_value FROM settings WHERE tenant_id = $1 AND setting_key = $2",
        )
        .bind(self.tenant_id)
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("Failed to get setting: {key}"))?;

        Ok(row.and_then(|r| r.get("setting_value")))
    }

    /// Set a setting value by key (upsert)
    pub async fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO settings (tenant_id, setting_key, setting_value, updated_at)
             VALUES ($1, $2, $3, now())
             ON CONFLICT (tenant_id, setting_key) DO UPDATE SET setting_value = $3, updated_at = now()",
        )
        .bind(self.tenant_id)
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await
        .with_context(|| format!("Failed to set setting: {key}"))?;
        Ok(())
    }

    /// Check if crawling is enabled.
    /// Returns the value from settings table, or defaults based on ENVIRONMENT:
    /// - dev: defaults to true (enabled)
    /// - prod: defaults to false (disabled for safety)
    pub async fn is_crawling_enabled(&self) -> Result<bool> {
        match self.get_setting("crawling_enabled").await? {
            Some(value) => Ok(value == "true"),
            None => {
                let is_dev = std::env::var("ENVIRONMENT")
                    .map(|v| v == "dev")
                    .unwrap_or(false);
                Ok(is_dev)
            }
        }
    }

    /// Set crawling enabled/disabled
    pub async fn set_crawling_enabled(&self, enabled: bool) -> Result<()> {
        let value = if enabled { "true" } else { "false" };
        self.set_setting("crawling_enabled", value).await
    }

    /// Get recently crawled pages for a domain, ordered by last_crawled_at descending.
    /// Returns tuples of (url, http_status, content_length, last_crawled_at).
    pub async fn get_recent_crawled_pages(
        &self,
        domain: &str,
        limit: i64,
    ) -> Result<Vec<(String, i32, i32, DateTime<Utc>)>> {
        let rows = sqlx::query(
            "SELECT url, http_status, content_length, last_crawled_at
             FROM crawled_pages
             WHERE tenant_id = $1 AND domain = $2
             ORDER BY last_crawled_at DESC
             LIMIT $3",
        )
        .bind(self.tenant_id)
        .bind(domain)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .with_context(|| format!("Failed to get recent crawled pages for domain: {domain}"))?;

        Ok(rows
            .iter()
            .map(|r| {
                let url: String = r.get("url");
                let http_status: i32 = r.get("http_status");
                let content_length: i32 = r.get("content_length");
                let last_crawled_at: DateTime<Utc> = r.get("last_crawled_at");
                (url, http_status, content_length, last_crawled_at)
            })
            .collect())
    }

    /// Re-queue an entry with incremented attempt count for retry.
    /// Schedules the retry with exponential backoff based on attempt count.
    pub async fn requeue_with_retry(&self, entry: &CrawlQueueEntry) -> Result<()> {
        let now = Utc::now();

        // Exponential backoff: 1min, 2min, 4min, 8min, etc.
        let backoff_minutes = 2i64.pow(entry.attempt_count as u32);
        let scheduled_at = now + chrono::Duration::minutes(backoff_minutes);

        let new_entry = CrawlQueueEntry {
            queue_id: Uuid::now_v7(),
            tenant_id: entry.tenant_id,
            priority: entry.priority + 1, // Lower priority for retries
            scheduled_at,
            url: entry.url.clone(),
            domain: entry.domain.clone(),
            last_attempt_at: Some(now),
            attempt_count: entry.attempt_count + 1,
            created_at: entry.created_at,
        };

        self.insert_queue_entry(&new_entry).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Integration tests requiring PostgreSQL.
    // Run with: cargo test -- --ignored
    // Requires DATABASE_URL env variable.

    /// Helper to create a DbClient from environment variables.
    async fn create_test_client() -> DbClient {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for tests");
        let pool = PgPool::connect(&database_url)
            .await
            .expect("Failed to connect to PostgreSQL");

        // Use a test tenant
        let tenant_id = Uuid::now_v7();
        let client = DbClient::new(pool, tenant_id);

        // Ensure the test tenant exists
        client
            .ensure_default_tenant(tenant_id, "Test Tenant")
            .await
            .expect("Failed to create test tenant");

        client
    }

    #[tokio::test]
    #[ignore]
    async fn test_postgres_connection() {
        let _client = create_test_client().await;
        // If we got here, connection succeeded
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_next_queue_entry() {
        let client = create_test_client().await;
        let result = client.get_next_queue_entry().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[ignore]
    async fn test_is_domain_allowed_returns_false_for_unlisted_domain() {
        let client = create_test_client().await;

        let test_domain = format!(
            "test-unlisted-{}.example.invalid",
            chrono::Utc::now().timestamp_millis()
        );

        let result = client.is_domain_allowed(&test_domain).await;
        assert!(result.is_ok());
        assert!(!result.unwrap(), "Domain should not be allowed");
    }

    #[tokio::test]
    #[ignore]
    async fn test_is_domain_allowed_returns_true_for_listed_domain() {
        let client = create_test_client().await;

        let test_domain = format!(
            "test-allowed-{}.example.invalid",
            chrono::Utc::now().timestamp_millis()
        );

        // Setup: Insert the test domain
        client
            .insert_allowed_domain(&test_domain, "test", Some("Test domain"))
            .await
            .expect("Failed to insert test domain");

        // Test: Check if domain is allowed
        let result = client.is_domain_allowed(&test_domain).await;
        assert!(result.is_ok());
        assert!(result.unwrap(), "Domain should be allowed");

        // Cleanup
        client
            .hard_delete_allowed_domain(&test_domain)
            .await
            .expect("Failed to clean up test domain");
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_and_set_setting() {
        let client = create_test_client().await;

        let test_key = format!("test_setting_{}", chrono::Utc::now().timestamp_millis());

        // Test: Initially no setting exists
        let result = client.get_setting(&test_key).await;
        assert!(result.is_ok());
        assert!(
            result.unwrap().is_none(),
            "Setting should not exist initially"
        );

        // Test: Set a setting value
        client
            .set_setting(&test_key, "test_value")
            .await
            .expect("Failed to set setting");

        // Test: Retrieve the setting
        let result = client.get_setting(&test_key).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some("test_value".to_string()));

        // Cleanup
        sqlx::query("DELETE FROM settings WHERE tenant_id = $1 AND setting_key = $2")
            .bind(client.tenant_id)
            .bind(&test_key)
            .execute(client.pool())
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_crawling_enabled_flag() {
        let client = create_test_client().await;

        client
            .set_crawling_enabled(true)
            .await
            .expect("Failed to enable crawling");

        let result = client.is_crawling_enabled().await;
        assert!(result.is_ok());
        assert!(result.unwrap(), "Crawling should be enabled");

        client
            .set_crawling_enabled(false)
            .await
            .expect("Failed to disable crawling");

        let result = client.is_crawling_enabled().await;
        assert!(result.is_ok());
        assert!(!result.unwrap(), "Crawling should be disabled");
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_recent_crawled_pages_returns_pages_sorted_by_crawled_at() {
        let client = create_test_client().await;

        let test_domain = format!(
            "test-recent-{}.example.invalid",
            chrono::Utc::now().timestamp_millis()
        );

        // Insert two crawled pages with different timestamps
        let now = Utc::now();
        let older_page = CrawledPage {
            page_id: Uuid::now_v7(),
            tenant_id: client.tenant_id,
            domain: test_domain.clone(),
            url_path: "/old".to_string(),
            url: format!("https://{}/old", test_domain),
            storage_id: None,
            storage_compression: CompressionType::None,
            last_crawled_at: now - chrono::Duration::hours(1),
            next_crawl_at: now + chrono::Duration::hours(24),
            crawl_frequency_hours: 24,
            http_status: 200,
            content_hash: "abc123".to_string(),
            content_length: 5000,
            robots_allowed: true,
            error_message: None,
            crawl_count: 1,
            created_at: now - chrono::Duration::hours(1),
            updated_at: now - chrono::Duration::hours(1),
        };
        let newer_page = CrawledPage {
            page_id: Uuid::now_v7(),
            tenant_id: client.tenant_id,
            domain: test_domain.clone(),
            url_path: "/new".to_string(),
            url: format!("https://{}/new", test_domain),
            last_crawled_at: now,
            content_hash: "def456".to_string(),
            content_length: 8000,
            created_at: now,
            updated_at: now,
            ..older_page.clone()
        };

        client
            .upsert_crawled_page(&older_page)
            .await
            .expect("Failed to insert older page");
        client
            .upsert_crawled_page(&newer_page)
            .await
            .expect("Failed to insert newer page");

        // Query recent pages
        let result = client
            .get_recent_crawled_pages(&test_domain, 10)
            .await
            .expect("Failed to get recent pages");

        assert_eq!(result.len(), 2);
        // First result should be newer (DESC order)
        assert!(result[0].0.contains("/new"));
        assert!(result[1].0.contains("/old"));
        assert_eq!(result[0].1, 200); // http_status
        assert_eq!(result[0].2, 8000); // content_length

        // Cleanup
        client
            .delete_crawled_page(&test_domain, "/old")
            .await
            .expect("Failed to clean up");
        client
            .delete_crawled_page(&test_domain, "/new")
            .await
            .expect("Failed to clean up");
    }

    #[tokio::test]
    #[ignore]
    async fn test_ensure_default_tenant_is_idempotent() {
        let client = create_test_client().await;

        // Should be idempotent - calling twice should not error
        client
            .ensure_default_tenant(client.tenant_id, "Test")
            .await
            .expect("First call should succeed");
        client
            .ensure_default_tenant(client.tenant_id, "Test")
            .await
            .expect("Second call should also succeed (ON CONFLICT DO NOTHING)");

        // Verify the tenant row exists
        let row = sqlx::query("SELECT tenant_id FROM tenants WHERE tenant_id = $1")
            .bind(client.tenant_id)
            .fetch_optional(client.pool())
            .await
            .unwrap();
        assert!(
            row.is_some(),
            "Tenant row should exist after ensure_default_tenant"
        );
    }
}
