// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use crate::models::db::{CrawlError, CrawlQueueEntry, CrawledPage};
use chrono::Timelike;
use scylla::frame::value::{Counter, CqlTimestamp};
use scylla::transport::errors::{NewSessionError, QueryError};
use scylla::{Session, SessionBuilder};
use std::sync::Arc;
use uuid::Uuid;

/// Apache Cassandra client for managing crawl queue and crawled pages
#[derive(Clone)]
pub struct CassandraClient {
    session: Arc<Session>,
    _keyspace: String,
}

impl CassandraClient {
    /// Create a new Apache Cassandra client
    pub async fn new(hosts: Vec<String>, keyspace: String) -> Result<Self, NewSessionError> {
        let session = SessionBuilder::new().known_nodes(&hosts).build().await?;

        let session = Arc::new(session);

        // Set the keyspace
        session.use_keyspace(&keyspace, false).await?;

        Ok(Self {
            session,
            _keyspace: keyspace,
        })
    }

    /// Get the next entry from the crawl queue (with lowest priority and earliest scheduled_at)
    /// This performs a SELECT to find entries to process
    pub async fn get_next_queue_entry(&self) -> Result<Option<CrawlQueueEntry>, QueryError> {
        let query =
            "SELECT priority, scheduled_at, url, domain, last_attempt_at, attempt_count, created_at
                     FROM crawl_queue
                     LIMIT 1";

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

        // Query crawl_stats for the current hour
        let query = "SELECT pages_crawled FROM crawl_stats
                     WHERE date = ? AND hour = ?";

        let result = self.session.query_unpaged(query, (date, hour)).await?;
        let rows_result = match result.into_rows_result() {
            Ok(r) => r,
            Err(_) => return Ok(0),
        };

        // Sum up all the counter values across domains
        let rows = rows_result.rows::<(Counter,)>().map_err(|e| {
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
            total_count += count.0;
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
        let query = "INSERT INTO crawl_queue
                     (priority, scheduled_at, url, domain, last_attempt_at, attempt_count, created_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?)";

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
        let query = "DELETE FROM crawl_queue
                     WHERE priority = ? AND scheduled_at = ? AND url = ?";

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
        let query = "INSERT INTO crawled_pages
                     (domain, url_path, url, storage_id, last_crawled_at, next_crawl_at,
                      crawl_frequency_hours, http_status, content_hash, content_length,
                      robots_allowed, error_message, crawl_count, created_at, updated_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)";

        self.session
            .query_unpaged(
                query,
                (
                    page.domain.as_str(),
                    page.url_path.as_str(),
                    page.url.as_str(),
                    page.storage_id,
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

        let query = "UPDATE crawl_stats
                     SET pages_crawled = pages_crawled + 1
                     WHERE date = ? AND hour = ? AND domain = ?";

        self.session
            .query_unpaged(query, (date, hour, domain))
            .await?;

        Ok(())
    }

    /// Check if a domain is in the allowed domains list
    /// Returns true if the domain is allowed, false otherwise
    pub async fn is_domain_allowed(&self, domain: &str) -> Result<bool, QueryError> {
        let query = "SELECT domain FROM allowed_domains WHERE domain = ?";

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
        let query = "SELECT domain, url_path, url, storage_id, last_crawled_at, next_crawl_at,
                            crawl_frequency_hours, http_status, content_hash, content_length,
                            robots_allowed, error_message, crawl_count, created_at, updated_at
                     FROM crawled_pages
                     WHERE domain = ? AND url_path = ?";

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
        let query = "INSERT INTO crawl_errors
                     (domain, occurred_at, url, error_type, error_message, attempt_count, stack_trace)
                     VALUES (?, ?, ?, ?, ?, ?, ?)";

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

        let query = "UPDATE crawl_stats
                     SET pages_failed = pages_failed + 1
                     WHERE date = ? AND hour = ? AND domain = ?";

        self.session
            .query_unpaged(query, (date, hour, domain))
            .await?;

        Ok(())
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

    // Note: These tests require a running Cassandra instance
    // They are integration tests and should be run with:
    // cargo test --test '*' -- --ignored

    #[tokio::test]
    #[ignore]
    async fn test_cassandra_connection() {
        let client =
            CassandraClient::new(vec!["127.0.0.1:9042".to_string()], "lalasearch".to_string())
                .await;

        assert!(client.is_ok());
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_next_queue_entry() {
        let client =
            CassandraClient::new(vec!["127.0.0.1:9042".to_string()], "lalasearch".to_string())
                .await
                .unwrap();

        let result = client.get_next_queue_entry().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[ignore]
    async fn test_is_domain_allowed_returns_false_for_unlisted_domain() {
        let client =
            CassandraClient::new(vec!["127.0.0.1:9042".to_string()], "lalasearch".to_string())
                .await
                .unwrap();

        // Test with a domain that's definitely not in the allowed list
        let result = client
            .is_domain_allowed("definitely-not-allowed-domain.example")
            .await;

        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[tokio::test]
    #[ignore]
    async fn test_is_domain_allowed_returns_true_for_listed_domain() {
        let client =
            CassandraClient::new(vec!["127.0.0.1:9042".to_string()], "lalasearch".to_string())
                .await
                .unwrap();

        // First, insert a test domain into allowed_domains
        let test_domain = "en.wikipedia.org";
        let insert_query =
            "INSERT INTO allowed_domains (domain, added_at, added_by, notes) VALUES (?, toTimestamp(now()), 'test', 'Test domain')";
        client
            .session
            .query_unpaged(insert_query, (test_domain,))
            .await
            .unwrap();

        // Now check if it's allowed
        let result = client.is_domain_allowed(test_domain).await;

        assert!(result.is_ok());
        assert!(result.unwrap());

        // Clean up: remove the test domain
        let delete_query = "DELETE FROM allowed_domains WHERE domain = ?";
        client
            .session
            .query_unpaged(delete_query, (test_domain,))
            .await
            .unwrap();
    }
}
