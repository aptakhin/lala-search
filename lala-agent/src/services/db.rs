// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use crate::models::db::{CrawlQueueEntry, CrawledPage};
use scylla::frame::value::CqlTimestamp;
use scylla::transport::errors::{NewSessionError, QueryError};
use scylla::{Session, SessionBuilder};
use std::sync::Arc;

/// ScyllaDB client for managing crawl queue and crawled pages
#[derive(Clone)]
pub struct ScyllaClient {
    session: Arc<Session>,
    _keyspace: String,
}

impl ScyllaClient {
    /// Create a new ScyllaDB client
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
    /// In ScyllaDB, we use DELETE rather than optimistic locking since the queue is designed
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
                     (domain, url_path, url, last_crawled_at, next_crawl_at,
                      crawl_frequency_hours, http_status, content_hash, content_length,
                      robots_allowed, error_message, crawl_count, created_at, updated_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)";

        self.session
            .query_unpaged(
                query,
                (
                    page.domain.as_str(),
                    page.url_path.as_str(),
                    page.url.as_str(),
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

        Ok(())
    }

    /// Get a crawled page by domain and url_path
    pub async fn get_crawled_page(
        &self,
        domain: &str,
        url_path: &str,
    ) -> Result<Option<CrawledPage>, QueryError> {
        let query = "SELECT domain, url_path, url, last_crawled_at, next_crawl_at,
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

        let rows_vec = rows_result
            .rows::<(
                String,
                String,
                String,
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

        if let Some(row_result) = rows_vec.into_iter().next() {
            let (
                domain,
                url_path,
                url,
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

            return Ok(Some(CrawledPage {
                domain,
                url_path,
                url,
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
            }));
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests require a running ScyllaDB instance
    // They are integration tests and should be run with:
    // cargo test --test '*' -- --ignored

    #[tokio::test]
    #[ignore]
    async fn test_scylla_connection() {
        let client =
            ScyllaClient::new(vec!["127.0.0.1:9042".to_string()], "lalasearch".to_string()).await;

        assert!(client.is_ok());
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_next_queue_entry() {
        let client =
            ScyllaClient::new(vec!["127.0.0.1:9042".to_string()], "lalasearch".to_string())
                .await
                .unwrap();

        let result = client.get_next_queue_entry().await;
        assert!(result.is_ok());
    }
}
