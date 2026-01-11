// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use chrono::{DateTime, Utc};
use scylla::frame::value::CqlTimestamp;

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
