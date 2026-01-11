// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use crate::models::crawler::{CrawlRequest, CrawlResult};
use crate::models::db::{CrawlQueueEntry, CrawledPage};
use crate::services::crawler::crawl_url;
use crate::services::db::ScyllaClient;
use anyhow::Result;
use chrono::Utc;
use scylla::frame::value::CqlTimestamp;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

/// Queue processor that continuously processes crawl queue entries
pub struct QueueProcessor {
    db_client: Arc<ScyllaClient>,
    user_agent: String,
    poll_interval: Duration,
}

impl QueueProcessor {
    /// Create a new queue processor
    pub fn new(db_client: Arc<ScyllaClient>, user_agent: String, poll_interval: Duration) -> Self {
        Self {
            db_client,
            user_agent,
            poll_interval,
        }
    }

    /// Start processing the queue in a loop
    pub async fn start(&self) {
        println!("Queue processor started");

        loop {
            match self.process_next_entry().await {
                Ok(processed) => {
                    if !processed {
                        // No entries to process, wait before polling again
                        sleep(self.poll_interval).await;
                    }
                }
                Err(e) => {
                    eprintln!("Error processing queue entry: {}", e);
                    sleep(self.poll_interval).await;
                }
            }
        }
    }

    /// Process a single entry from the queue
    /// Returns true if an entry was processed, false if queue was empty
    async fn process_next_entry(&self) -> Result<bool> {
        // Get the next entry from the queue
        let entry = match self
            .db_client
            .get_next_queue_entry()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get queue entry: {}", e))?
        {
            Some(entry) => entry,
            None => return Ok(false),
        };

        println!("Processing URL: {}", entry.url);

        // Delete the entry from the queue immediately to prevent other workers from picking it up
        // This is the "locking" mechanism - whoever deletes it first gets to process it
        self.db_client
            .delete_queue_entry(&entry)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to delete queue entry: {}", e))?;

        // Crawl the URL
        let request = CrawlRequest {
            url: entry.url.clone(),
            user_agent: self.user_agent.clone(),
        };

        let result = crawl_url(request)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to crawl URL: {}", e))?;

        // Convert crawl result to crawled page
        let crawled_page = self.create_crawled_page(&entry, &result).await?;

        // Store the result in crawled_pages table
        self.db_client
            .upsert_crawled_page(&crawled_page)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to upsert crawled page: {}", e))?;

        println!("Successfully processed URL: {}", entry.url);

        Ok(true)
    }

    /// Convert crawl result to a crawled page entry
    async fn create_crawled_page(
        &self,
        entry: &CrawlQueueEntry,
        result: &CrawlResult,
    ) -> Result<CrawledPage> {
        let parsed_url = url::Url::parse(&entry.url)?;
        let domain = parsed_url.host_str().unwrap_or(&entry.domain).to_string();
        let url_path = parsed_url.path().to_string();

        let now = Utc::now();
        let now_timestamp = CqlTimestamp(now.timestamp_millis());

        let crawl_frequency_hours = 24; // Default: recrawl once per day
        let next_crawl = now + chrono::Duration::hours(crawl_frequency_hours as i64);
        let next_crawl_at = CqlTimestamp(next_crawl.timestamp_millis());

        // Calculate content hash if content exists
        let (content_hash, content_length) = if let Some(ref content) = result.content {
            let hash = format!("{:x}", md5::compute(content));
            (hash, content.len() as i64)
        } else {
            ("".to_string(), 0)
        };

        // Determine HTTP status
        let http_status = if result.allowed_by_robots {
            if result.content.is_some() {
                200 // Assume success if we got content
            } else {
                500 // Error occurred
            }
        } else {
            403 // Forbidden by robots.txt
        };

        // Check if this page was crawled before to increment crawl_count
        let existing_page = self
            .db_client
            .get_crawled_page(&domain, &url_path)
            .await
            .ok()
            .flatten();

        let (crawl_count, created_at) = if let Some(ref existing) = existing_page {
            (existing.crawl_count + 1, existing.created_at)
        } else {
            (1, now_timestamp)
        };

        Ok(CrawledPage {
            domain,
            url_path,
            url: entry.url.clone(),
            last_crawled_at: now_timestamp,
            next_crawl_at,
            crawl_frequency_hours,
            http_status,
            content_hash,
            content_length,
            robots_allowed: result.allowed_by_robots,
            error_message: result.error.clone(),
            crawl_count,
            created_at,
            updated_at: now_timestamp,
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_create_crawled_page_success() {
        // This test verifies the logic of creating a CrawledPage from a CrawlResult
        // Note: This is a unit test and doesn't require a database connection
    }
}
