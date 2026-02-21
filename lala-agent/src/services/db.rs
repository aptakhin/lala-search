// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use crate::models::db::{CrawlError, CrawlQueueEntry, CrawledPage};
use crate::models::storage::CompressionType;
use anyhow::{anyhow, Result};
use chrono::Timelike;
use scylla::frame::value::{Counter, CqlTimestamp};
use scylla::transport::errors::{NewSessionError, QueryError};
use scylla::{Session, SessionBuilder};
use std::sync::Arc;
use uuid::Uuid;

/// Configuration for Cassandra database connection
#[derive(Debug, Clone)]
pub struct CassandraConfig {
    pub hosts: Vec<String>,
    pub keyspace: String,
}

impl CassandraConfig {
    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self> {
        let hosts_str = std::env::var("CASSANDRA_HOSTS")
            .map_err(|_| anyhow!("CASSANDRA_HOSTS environment variable not set"))?;
        let hosts: Vec<String> = hosts_str.split(',').map(|s| s.trim().to_string()).collect();

        let keyspace = std::env::var("CASSANDRA_KEYSPACE")
            .map_err(|_| anyhow!("CASSANDRA_KEYSPACE environment variable not set"))?;

        Ok(Self { hosts, keyspace })
    }
}

/// Apache Cassandra client for managing crawl queue and crawled pages.
///
/// All queries use fully qualified table names (`keyspace.table`) so that multiple
/// instances can share the same connection pool while targeting different keyspaces.
/// This is the foundation for multi-tenant operation: call `with_keyspace()` to create
/// a tenant-scoped client that reuses the same underlying connections.
#[derive(Clone)]
pub struct CassandraClient {
    session: Arc<Session>,
    pub keyspace: String,
}

impl CassandraClient {
    /// Create a new Apache Cassandra client.
    ///
    /// Does not issue a USE statement. All queries use fully qualified table names
    /// so the same connection pool can serve multiple keyspaces concurrently.
    pub async fn new(hosts: Vec<String>, keyspace: String) -> Result<Self, NewSessionError> {
        let session = SessionBuilder::new().known_nodes(&hosts).build().await?;

        Ok(Self {
            session: Arc::new(session),
            keyspace,
        })
    }

    /// Create a new Apache Cassandra client from configuration
    pub async fn from_config(config: CassandraConfig) -> Result<Self, NewSessionError> {
        Self::new(config.hosts, config.keyspace).await
    }

    /// Create a new client targeting a different keyspace, sharing the same connection pool.
    ///
    /// Used in multi-tenant mode to scope a request to a specific tenant's keyspace:
    /// ```rust,ignore
    /// let tenant_db = state.db_client.with_keyspace("lalasearch_acme");
    /// tenant_db.is_domain_allowed(&domain).await?;
    /// ```
    pub fn with_keyspace(&self, keyspace: impl Into<String>) -> Self {
        Self {
            session: self.session.clone(),
            keyspace: keyspace.into(),
        }
    }

    /// Get a reference to the underlying Scylla session.
    /// Used for creating specialized clients like AuthDbClient.
    pub fn session(&self) -> Arc<Session> {
        self.session.clone()
    }

    /// Ensure the default tenant row exists in the system keyspace.
    ///
    /// `tenant_keyspace` is the Cassandra keyspace name used as the tenant_id
    /// (e.g. `lalasearch_default` or `lalasearch_test`).
    /// Uses IF NOT EXISTS so it is safe to call repeatedly.
    pub async fn ensure_default_tenant(&self, tenant_keyspace: &str) -> Result<(), QueryError> {
        let query = format!(
            "INSERT INTO {}.tenants (tenant_id, name, created_at) \
             VALUES (?, 'Default', toTimestamp(now())) IF NOT EXISTS",
            self.keyspace
        );
        self.session
            .query_unpaged(query, (tenant_keyspace,))
            .await?;
        Ok(())
    }

    /// List all tenant keyspace names registered in the system keyspace's tenants table.
    ///
    /// In multi-tenant mode the scheduler calls this on startup to discover which
    /// Cassandra keyspaces need a queue processor.  Each `tenant_id` value in the
    /// table **is** the Cassandra keyspace name (e.g. `lalasearch_acme`).
    pub async fn list_tenant_keyspaces(&self) -> Result<Vec<String>, QueryError> {
        let query = format!("SELECT tenant_id FROM {}.tenants", self.keyspace);
        let result = self.session.query_unpaged(query, &[]).await?;
        let rows_result = match result.into_rows_result() {
            Ok(r) => r,
            Err(_) => return Ok(Vec::new()),
        };

        let rows = rows_result.rows::<(String,)>().map_err(|e| {
            QueryError::DbError(
                scylla::transport::errors::DbError::Other(0),
                format!("Failed to deserialize tenants rows: {}", e),
            )
        })?;

        let mut keyspaces = Vec::new();
        for row_result in rows {
            let (tenant_id,) = row_result.map_err(|e| {
                QueryError::DbError(
                    scylla::transport::errors::DbError::Other(0),
                    format!("Failed to parse tenant row: {}", e),
                )
            })?;
            keyspaces.push(tenant_id);
        }
        Ok(keyspaces)
    }

    /// Insert an allowed domain
    pub async fn insert_allowed_domain(
        &self,
        domain: &str,
        added_by: &str,
        notes: Option<&str>,
    ) -> Result<(), QueryError> {
        let query = format!(
            "INSERT INTO {}.allowed_domains (domain, added_at, added_by, notes) VALUES (?, toTimestamp(now()), ?, ?)",
            self.keyspace
        );
        self.session
            .query_unpaged(query, (domain, added_by, notes))
            .await?;
        Ok(())
    }

    /// Delete an allowed domain (used for test cleanup)
    pub async fn delete_allowed_domain(&self, domain: &str) -> Result<(), QueryError> {
        let query = format!(
            "DELETE FROM {}.allowed_domains WHERE domain = ?",
            self.keyspace
        );
        self.session.query_unpaged(query, (domain,)).await?;
        Ok(())
    }

    /// List all allowed domains
    pub async fn list_allowed_domains(
        &self,
    ) -> Result<Vec<(String, Option<String>, Option<String>, Option<String>)>, QueryError> {
        let query = format!(
            "SELECT domain, added_by, notes, added_at FROM {}.allowed_domains",
            self.keyspace
        );
        let result = self.session.query_unpaged(query, &[]).await?;
        let rows_result = match result.into_rows_result() {
            Ok(rows_result) => rows_result,
            Err(_) => return Ok(Vec::new()),
        };

        let rows_vec = rows_result
            .rows::<(String, Option<String>, Option<String>, Option<CqlTimestamp>)>()
            .map_err(|e| {
                QueryError::DbError(
                    scylla::transport::errors::DbError::Other(0),
                    format!("Failed to deserialize allowed domains: {}", e),
                )
            })?;

        let mut domains = Vec::new();
        for row in rows_vec {
            let (domain, added_by, notes, added_at) = row.map_err(|e| {
                QueryError::DbError(
                    scylla::transport::errors::DbError::Other(0),
                    format!("Failed to parse allowed domain row: {}", e),
                )
            })?;

            // Convert CqlTimestamp to String if present
            let added_at_str = added_at.map(|ts| {
                let millis = ts.0;
                let datetime = chrono::DateTime::from_timestamp_millis(millis)
                    .unwrap_or_else(chrono::Utc::now);
                datetime.to_rfc3339()
            });

            domains.push((domain, added_by, notes, added_at_str));
        }

        Ok(domains)
    }

    /// Delete a crawled page by domain and url_path (used for test cleanup)
    pub async fn delete_crawled_page(
        &self,
        domain: &str,
        url_path: &str,
    ) -> Result<(), QueryError> {
        let query = format!(
            "DELETE FROM {}.crawled_pages WHERE domain = ? AND url_path = ?",
            self.keyspace
        );
        self.session
            .query_unpaged(query, (domain, url_path))
            .await?;
        Ok(())
    }

    /// Get the next entry from the crawl queue (with lowest priority and earliest scheduled_at)
    /// This performs a SELECT to find entries to process
    pub async fn get_next_queue_entry(&self) -> Result<Option<CrawlQueueEntry>, QueryError> {
        let query = format!(
            "SELECT priority, scheduled_at, url, domain, last_attempt_at, attempt_count, created_at
             FROM {}.crawl_queue
             LIMIT 1",
            self.keyspace
        );

        let result = self.session.query_unpaged(query, &[]).await?;
        let rows_result = match result.into_rows_result() {
            Ok(rows_result) => rows_result,
            Err(_) => return Ok(None),
        };

        let rows_vec = rows_result
            .rows::<(
                i32,
                CqlTimestamp,
                String,
                String,
                Option<CqlTimestamp>,
                i32,
                CqlTimestamp,
            )>()
            .map_err(|e| {
                QueryError::DbError(
                    scylla::transport::errors::DbError::Other(0),
                    format!("Failed to deserialize rows: {}", e),
                )
            })?;

        if let Some(row_result) = rows_vec.into_iter().next() {
            let (priority, scheduled_at, url, domain, last_attempt_at, attempt_count, created_at) =
                row_result.map_err(|e| {
                    QueryError::DbError(
                        scylla::transport::errors::DbError::Other(0),
                        format!("Failed to parse queue entry: {}", e),
                    )
                })?;

            return Ok(Some(CrawlQueueEntry {
                priority,
                scheduled_at,
                url,
                domain,
                last_attempt_at,
                attempt_count,
                created_at,
            }));
        }

        Ok(None)
    }

    /// Return total number of crawled pages from crawl_stats.
    /// This queries the counter table instead of doing a full table scan.
    /// Returns the sum of pages_crawled across all domains for today.
    pub async fn count_crawled_pages(&self) -> Result<i64, QueryError> {
        // Get current date and hour for the partition key
        let now = chrono::Utc::now();
        let date = now.date_naive();
        let hour = now.hour() as i32;

        let query = format!(
            "SELECT pages_crawled FROM {}.crawl_stats WHERE date = ? AND hour = ?",
            self.keyspace
        );
        let result = self.session.query_unpaged(query, (date, hour)).await?;
        let rows_result = match result.into_rows_result() {
            Ok(r) => r,
            Err(_) => return Ok(0),
        };

        // Sum up all the counter values across domains
        // Note: Counter columns can be NULL until first incremented, so we use Option<Counter>
        let rows = rows_result.rows::<(Option<Counter>,)>().map_err(|e| {
            QueryError::DbError(
                scylla::transport::errors::DbError::Other(0),
                format!("Failed to deserialize count rows: {}", e),
            )
        })?;

        let mut total_count = 0i64;
        for row_res in rows {
            let (count,) = row_res.map_err(|e| {
                QueryError::DbError(
                    scylla::transport::errors::DbError::Other(0),
                    format!("Failed to parse count row: {}", e),
                )
            })?;
            // Treat NULL counters as 0
            total_count += count.map(|c| c.0).unwrap_or(0);
        }

        Ok(total_count)
    }

    /// Convenience method to check if a crawled page exists by domain + url_path
    pub async fn crawled_page_exists(
        &self,
        domain: &str,
        url_path: &str,
    ) -> Result<bool, QueryError> {
        // Reuse get_crawled_page implementation
        match self.get_crawled_page(domain, url_path).await {
            Ok(Some(_)) => Ok(true),
            Ok(None) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Insert an entry into the crawl queue
    pub async fn insert_queue_entry(&self, entry: &CrawlQueueEntry) -> Result<(), QueryError> {
        let query = format!(
            "INSERT INTO {}.crawl_queue
             (priority, scheduled_at, url, domain, last_attempt_at, attempt_count, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            self.keyspace
        );

        self.session
            .query_unpaged(
                query,
                (
                    entry.priority,
                    entry.scheduled_at,
                    entry.url.as_str(),
                    entry.domain.as_str(),
                    entry.last_attempt_at,
                    entry.attempt_count,
                    entry.created_at,
                ),
            )
            .await?;

        Ok(())
    }

    /// Delete an entry from the crawl queue
    /// In Cassandra, we use DELETE rather than optimistic locking since the queue is designed
    /// for multiple workers. If the entry was already processed by another worker, the DELETE
    /// will simply affect 0 rows.
    pub async fn delete_queue_entry(&self, entry: &CrawlQueueEntry) -> Result<(), QueryError> {
        let query = format!(
            "DELETE FROM {}.crawl_queue WHERE priority = ? AND scheduled_at = ? AND url = ?",
            self.keyspace
        );

        self.session
            .query_unpaged(
                query,
                (entry.priority, entry.scheduled_at, entry.url.as_str()),
            )
            .await?;

        Ok(())
    }

    /// Insert or update a crawled page
    pub async fn upsert_crawled_page(&self, page: &CrawledPage) -> Result<(), QueryError> {
        let query = format!(
            "INSERT INTO {}.crawled_pages
             (domain, url_path, url, storage_id, storage_compression, last_crawled_at, next_crawl_at,
              crawl_frequency_hours, http_status, content_hash, content_length,
              robots_allowed, error_message, crawl_count, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            self.keyspace
        );

        self.session
            .query_unpaged(
                query,
                (
                    page.domain.as_str(),
                    page.url_path.as_str(),
                    page.url.as_str(),
                    page.storage_id,
                    page.storage_compression.to_db_value(),
                    page.last_crawled_at,
                    page.next_crawl_at,
                    page.crawl_frequency_hours,
                    page.http_status,
                    page.content_hash.as_str(),
                    page.content_length,
                    page.robots_allowed,
                    page.error_message.as_deref(),
                    page.crawl_count,
                    page.created_at,
                    page.updated_at,
                ),
            )
            .await?;

        // Increment crawl_stats counter
        self.increment_crawl_stats(&page.domain).await?;

        Ok(())
    }

    /// Increment the crawl_stats counter for pages_crawled
    async fn increment_crawl_stats(&self, domain: &str) -> Result<(), QueryError> {
        // Get current date and hour for partitioning
        let now = chrono::Utc::now();
        let date = now.date_naive();
        let hour = now.hour() as i32;

        let query = format!(
            "UPDATE {}.crawl_stats SET pages_crawled = pages_crawled + 1 WHERE date = ? AND hour = ? AND domain = ?",
            self.keyspace
        );

        self.session
            .query_unpaged(query, (date, hour, domain))
            .await?;

        Ok(())
    }

    /// Check if a domain is in the allowed domains list
    /// Returns true if the domain is allowed, false otherwise
    pub async fn is_domain_allowed(&self, domain: &str) -> Result<bool, QueryError> {
        let query = format!(
            "SELECT domain FROM {}.allowed_domains WHERE domain = ?",
            self.keyspace
        );

        let result = self.session.query_unpaged(query, (domain,)).await?;
        let rows_result = match result.into_rows_result() {
            Ok(rows_result) => rows_result,
            Err(_) => return Ok(false),
        };

        let mut rows_iter = rows_result.rows::<(String,)>().map_err(|e| {
            QueryError::DbError(
                scylla::transport::errors::DbError::Other(0),
                format!("Failed to parse allowed domain row: {}", e),
            )
        })?;

        // If there's at least one row, the domain is allowed
        Ok(rows_iter.next().is_some())
    }

    /// Get a crawled page by domain and url_path
    pub async fn get_crawled_page(
        &self,
        domain: &str,
        url_path: &str,
    ) -> Result<Option<CrawledPage>, QueryError> {
        let query = format!(
            "SELECT domain, url_path, url, storage_id, storage_compression, last_crawled_at, next_crawl_at,
                    crawl_frequency_hours, http_status, content_hash, content_length,
                    robots_allowed, error_message, crawl_count, created_at, updated_at
             FROM {}.crawled_pages
             WHERE domain = ? AND url_path = ?",
            self.keyspace
        );

        let result = self
            .session
            .query_unpaged(query, (domain, url_path))
            .await?;
        let rows_result = match result.into_rows_result() {
            Ok(rows_result) => rows_result,
            Err(_) => return Ok(None),
        };

        Self::parse_crawled_page_row(rows_result)
    }

    /// Parse a single crawled page from query result rows
    fn parse_crawled_page_row(
        rows_result: scylla::QueryRowsResult,
    ) -> Result<Option<CrawledPage>, QueryError> {
        let rows_vec = rows_result
            .rows::<(
                String,
                String,
                String,
                Option<Uuid>,
                Option<i8>, // storage_compression can be NULL for old rows
                CqlTimestamp,
                CqlTimestamp,
                i32,
                i32,
                String,
                i64,
                bool,
                Option<String>,
                i32,
                CqlTimestamp,
                CqlTimestamp,
            )>()
            .map_err(|e| {
                QueryError::DbError(
                    scylla::transport::errors::DbError::Other(0),
                    format!("Failed to deserialize rows: {}", e),
                )
            })?;

        let Some(row_result) = rows_vec.into_iter().next() else {
            return Ok(None);
        };

        let (
            domain,
            url_path,
            url,
            storage_id,
            storage_compression_value,
            last_crawled_at,
            next_crawl_at,
            crawl_frequency_hours,
            http_status,
            content_hash,
            content_length,
            robots_allowed,
            error_message,
            crawl_count,
            created_at,
            updated_at,
        ) = row_result.map_err(|e| {
            QueryError::DbError(
                scylla::transport::errors::DbError::Other(0),
                format!("Failed to parse crawled page: {}", e),
            )
        })?;

        Ok(Some(CrawledPage {
            domain,
            url_path,
            url,
            storage_id,
            storage_compression: CompressionType::from_db_value(storage_compression_value),
            last_crawled_at,
            next_crawl_at,
            crawl_frequency_hours,
            http_status,
            content_hash,
            content_length,
            robots_allowed,
            error_message,
            crawl_count,
            created_at,
            updated_at,
        }))
    }

    /// Log a crawl error to the crawl_errors table
    pub async fn log_crawl_error(&self, error: &CrawlError) -> Result<(), QueryError> {
        let query = format!(
            "INSERT INTO {}.crawl_errors
             (domain, occurred_at, url, error_type, error_message, attempt_count, stack_trace)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            self.keyspace
        );

        self.session
            .query_unpaged(
                query,
                (
                    error.domain.as_str(),
                    error.occurred_at,
                    error.url.as_str(),
                    error.error_type.to_string(),
                    error.error_message.as_str(),
                    error.attempt_count,
                    error.stack_trace.as_deref(),
                ),
            )
            .await?;

        // Also increment the failed counter in crawl_stats
        self.increment_crawl_failed_stats(&error.domain).await?;

        Ok(())
    }

    /// Increment the crawl_stats counter for pages_failed
    async fn increment_crawl_failed_stats(&self, domain: &str) -> Result<(), QueryError> {
        let now = chrono::Utc::now();
        let date = now.date_naive();
        let hour = now.hour() as i32;

        let query = format!(
            "UPDATE {}.crawl_stats SET pages_failed = pages_failed + 1 WHERE date = ? AND hour = ? AND domain = ?",
            self.keyspace
        );

        self.session
            .query_unpaged(query, (date, hour, domain))
            .await?;

        Ok(())
    }

    // ========== Settings Methods ==========

    /// Get a setting value by key
    pub async fn get_setting(&self, key: &str) -> Result<Option<String>, QueryError> {
        let query = format!(
            "SELECT setting_value FROM {}.settings WHERE setting_key = ?",
            self.keyspace
        );
        let result = self.session.query_unpaged(query, (key,)).await?;
        let rows_result = match result.into_rows_result() {
            Ok(rows_result) => rows_result,
            Err(_) => return Ok(None),
        };

        let mut rows_iter = rows_result.rows::<(Option<String>,)>().map_err(|e| {
            QueryError::DbError(
                scylla::transport::errors::DbError::Other(0),
                format!("Failed to parse setting row: {}", e),
            )
        })?;

        if let Some(row_result) = rows_iter.next() {
            let (value,) = row_result.map_err(|e| {
                QueryError::DbError(
                    scylla::transport::errors::DbError::Other(0),
                    format!("Failed to parse setting value: {}", e),
                )
            })?;
            return Ok(value);
        }

        Ok(None)
    }

    /// Set a setting value by key
    pub async fn set_setting(&self, key: &str, value: &str) -> Result<(), QueryError> {
        let query = format!(
            "INSERT INTO {}.settings (setting_key, setting_value, updated_at) VALUES (?, ?, toTimestamp(now()))",
            self.keyspace
        );
        self.session.query_unpaged(query, (key, value)).await?;
        Ok(())
    }

    /// Check if crawling is enabled
    /// Returns the value from settings table, or defaults based on ENVIRONMENT:
    /// - dev: defaults to true (enabled)
    /// - prod: defaults to false (disabled for safety)
    pub async fn is_crawling_enabled(&self) -> Result<bool, QueryError> {
        match self.get_setting("crawling_enabled").await? {
            Some(value) => Ok(value == "true"),
            None => {
                // No setting in DB - use environment-based default
                let is_dev = std::env::var("ENVIRONMENT")
                    .map(|v| v == "dev")
                    .unwrap_or(false);
                Ok(is_dev)
            }
        }
    }

    /// Set crawling enabled/disabled
    pub async fn set_crawling_enabled(&self, enabled: bool) -> Result<(), QueryError> {
        let value = if enabled { "true" } else { "false" };
        self.set_setting("crawling_enabled", value).await
    }

    /// Re-queue an entry with incremented attempt count for retry
    /// Schedules the retry with exponential backoff based on attempt count
    pub async fn requeue_with_retry(&self, entry: &CrawlQueueEntry) -> Result<(), QueryError> {
        let now = chrono::Utc::now();

        // Exponential backoff: 1min, 2min, 4min, 8min, etc.
        let backoff_minutes = 2i64.pow(entry.attempt_count as u32);
        let scheduled_at = now + chrono::Duration::minutes(backoff_minutes);

        let new_entry = CrawlQueueEntry {
            priority: entry.priority + 1, // Lower priority for retries
            scheduled_at: CqlTimestamp(scheduled_at.timestamp_millis()),
            url: entry.url.clone(),
            domain: entry.domain.clone(),
            last_attempt_at: Some(CqlTimestamp(now.timestamp_millis())),
            attempt_count: entry.attempt_count + 1,
            created_at: entry.created_at,
        };

        self.insert_queue_entry(&new_entry).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Integration tests requiring Cassandra.
    // Run with: cargo test -- --ignored
    // Requires CASSANDRA_HOSTS, CASSANDRA_KEYSPACE, and CASSANDRA_SYSTEM_KEYSPACE env variables.

    /// Helper to create a CassandraClient for the tenant keyspace from environment variables.
    async fn create_test_client() -> CassandraClient {
        let config = CassandraConfig::from_env()
            .expect("CASSANDRA_HOSTS and CASSANDRA_KEYSPACE must be set");
        CassandraClient::from_config(config)
            .await
            .expect("Failed to connect to Cassandra")
    }

    /// Helper to create a CassandraClient for the system keyspace from environment variables.
    async fn create_system_test_client() -> CassandraClient {
        let hosts_str = std::env::var("CASSANDRA_HOSTS").expect("CASSANDRA_HOSTS must be set");
        let hosts: Vec<String> = hosts_str.split(',').map(|s| s.trim().to_string()).collect();
        let system_keyspace = std::env::var("CASSANDRA_SYSTEM_KEYSPACE")
            .expect("CASSANDRA_SYSTEM_KEYSPACE must be set");
        CassandraClient::new(hosts, system_keyspace)
            .await
            .expect("Failed to connect to Cassandra")
    }

    #[tokio::test]
    #[ignore]
    async fn test_cassandra_connection() {
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

        // Use a unique domain name to avoid collision with any real data
        let test_domain = format!(
            "test-unlisted-{}.example.invalid",
            chrono::Utc::now().timestamp_millis()
        );

        // Ensure domain is NOT in allowed list (clean state)
        client.delete_allowed_domain(&test_domain).await.ok();

        let result = client.is_domain_allowed(&test_domain).await;

        assert!(result.is_ok());
        assert!(!result.unwrap(), "Domain should not be allowed");
    }

    #[tokio::test]
    #[ignore]
    async fn test_is_domain_allowed_returns_true_for_listed_domain() {
        let client = create_test_client().await;

        // Use a unique domain name to ensure test isolation
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

        // Cleanup: Remove the test domain
        client
            .delete_allowed_domain(&test_domain)
            .await
            .expect("Failed to clean up test domain");
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_and_set_setting() {
        let client = create_test_client().await;

        // Use a unique setting key to avoid collision
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

        // Cleanup: Delete the test setting
        let delete_query = format!(
            "DELETE FROM {}.settings WHERE setting_key = ?",
            client.keyspace
        );
        client
            .session
            .query_unpaged(delete_query, (&test_key,))
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_crawling_enabled_flag() {
        let client = create_test_client().await;

        // Set crawling to enabled
        client
            .set_crawling_enabled(true)
            .await
            .expect("Failed to enable crawling");

        let result = client.is_crawling_enabled().await;
        assert!(result.is_ok());
        assert!(result.unwrap(), "Crawling should be enabled");

        // Set crawling to disabled
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
    async fn test_ensure_default_tenant_is_idempotent() {
        let system_client = create_system_test_client().await;
        let tenant_ks = std::env::var("CASSANDRA_KEYSPACE")
            .unwrap_or_else(|_| "lalasearch_default".to_string());

        // Should be idempotent - calling twice should not error
        system_client
            .ensure_default_tenant(&tenant_ks)
            .await
            .expect("First call should succeed");
        system_client
            .ensure_default_tenant(&tenant_ks)
            .await
            .expect("Second call should also succeed (IF NOT EXISTS)");

        // Verify the tenant row exists
        let query = format!(
            "SELECT tenant_id FROM {}.tenants WHERE tenant_id = ?",
            system_client.keyspace
        );
        let result = system_client
            .session
            .query_unpaged(query, (&tenant_ks,))
            .await
            .unwrap();
        let rows_result = result.into_rows_result().unwrap();
        let mut rows = rows_result.rows::<(String,)>().unwrap();
        assert!(
            rows.next().is_some(),
            "Tenant row should exist after ensure_default_tenant"
        );
    }
}
