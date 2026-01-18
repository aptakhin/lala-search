// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use crate::models::storage::CompressionType;
use chrono::{DateTime, Utc};
use scylla::frame::value::CqlTimestamp;
use uuid::Uuid;

/// Entry in the crawl queue
#[derive(Debug, Clone)]
pub struct CrawlQueueEntry {
    pub priority: i32,
    pub scheduled_at: CqlTimestamp,
    pub url: String,
    pub domain: String,
    pub last_attempt_at: Option<CqlTimestamp>,
    pub attempt_count: i32,
    pub created_at: CqlTimestamp,
}

impl CrawlQueueEntry {
    /// Convert CqlTimestamp to DateTime<Utc>
    pub fn scheduled_at_datetime(&self) -> DateTime<Utc> {
        chrono::DateTime::from_timestamp_millis(self.scheduled_at.0).unwrap_or_else(Utc::now)
    }
}

/// Crawled page metadata stored in the database
#[derive(Debug, Clone)]
pub struct CrawledPage {
    pub domain: String,
    pub url_path: String,
    pub url: String,
    pub storage_id: Option<Uuid>,
    pub storage_compression: CompressionType,
    pub last_crawled_at: CqlTimestamp,
    pub next_crawl_at: CqlTimestamp,
    pub crawl_frequency_hours: i32,
    pub http_status: i32,
    pub content_hash: String,
    pub content_length: i64,
    pub robots_allowed: bool,
    pub error_message: Option<String>,
    pub crawl_count: i32,
    pub created_at: CqlTimestamp,
    pub updated_at: CqlTimestamp,
}

/// Error types for crawl operations
#[derive(Debug, Clone, PartialEq)]
pub enum CrawlErrorType {
    /// Failed to fetch the URL (network error, timeout, etc.)
    FetchError,
    /// Failed to upload content to S3 storage
    StorageError,
    /// Failed to save to Cassandra database
    DatabaseError,
    /// Failed to index in search engine
    SearchIndexError,
    /// robots.txt disallowed crawling
    RobotsDisallowed,
    /// URL parsing or validation error
    InvalidUrl,
    /// Unknown or unexpected error
    Unknown,
}

impl std::fmt::Display for CrawlErrorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CrawlErrorType::FetchError => write!(f, "fetch_error"),
            CrawlErrorType::StorageError => write!(f, "storage_error"),
            CrawlErrorType::DatabaseError => write!(f, "database_error"),
            CrawlErrorType::SearchIndexError => write!(f, "search_index_error"),
            CrawlErrorType::RobotsDisallowed => write!(f, "robots_disallowed"),
            CrawlErrorType::InvalidUrl => write!(f, "invalid_url"),
            CrawlErrorType::Unknown => write!(f, "unknown"),
        }
    }
}

/// Crawl error record for observability
#[derive(Debug, Clone)]
pub struct CrawlError {
    pub domain: String,
    pub occurred_at: CqlTimestamp,
    pub url: String,
    pub error_type: CrawlErrorType,
    pub error_message: String,
    pub attempt_count: i32,
    pub stack_trace: Option<String>,
}
