// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use crate::models::storage::CompressionType;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Entry in the crawl queue
#[derive(Debug, Clone)]
pub struct CrawlQueueEntry {
    pub queue_id: Uuid,
    pub tenant_id: Uuid,
    pub priority: i32,
    pub scheduled_at: DateTime<Utc>,
    pub url: String,
    pub domain: String,
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub attempt_count: i32,
    pub created_at: DateTime<Utc>,
}

/// Crawled page metadata stored in the database
#[derive(Debug, Clone)]
pub struct CrawledPage {
    pub page_id: Uuid,
    pub tenant_id: Uuid,
    pub domain: String,
    pub url_path: String,
    pub url: String,
    pub storage_id: Option<Uuid>,
    pub storage_compression: CompressionType,
    pub last_crawled_at: DateTime<Utc>,
    pub next_crawl_at: DateTime<Utc>,
    pub crawl_frequency_hours: i32,
    pub http_status: i32,
    pub content_hash: String,
    pub content_length: i32,
    pub indexed_document_bytes: Option<i64>,
    pub robots_allowed: bool,
    pub error_message: Option<String>,
    pub crawl_count: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Error types for crawl operations
#[derive(Debug, Clone, PartialEq)]
pub enum CrawlErrorType {
    /// Failed to fetch the URL (network error, timeout, etc.)
    FetchError,
    /// Failed to upload content to S3 storage
    StorageError,
    /// Failed to save to database
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

impl CrawlErrorType {
    pub fn parse(s: &str) -> Self {
        match s {
            "fetch_error" => CrawlErrorType::FetchError,
            "storage_error" => CrawlErrorType::StorageError,
            "database_error" => CrawlErrorType::DatabaseError,
            "search_index_error" => CrawlErrorType::SearchIndexError,
            "robots_disallowed" => CrawlErrorType::RobotsDisallowed,
            "invalid_url" => CrawlErrorType::InvalidUrl,
            _ => CrawlErrorType::Unknown,
        }
    }
}

/// Crawl error record for observability
#[derive(Debug, Clone)]
pub struct CrawlError {
    pub error_id: Uuid,
    pub tenant_id: Uuid,
    pub page_id: Option<Uuid>,
    pub domain: String,
    pub occurred_at: DateTime<Utc>,
    pub url: String,
    pub error_type: CrawlErrorType,
    pub error_message: String,
    pub attempt_count: i32,
    pub stack_trace: Option<String>,
}
