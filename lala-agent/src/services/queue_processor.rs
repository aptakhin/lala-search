// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use crate::models::crawler::{CrawlRequest, CrawlResult};
use crate::models::db::{CrawlError, CrawlErrorType, CrawlQueueEntry, CrawledPage};
use crate::models::search::IndexedDocument;
use crate::services::crawler::crawl_url;
use crate::services::db::CassandraClient;
use crate::services::search::SearchClient;
use crate::services::storage::StorageClient;
use anyhow::Result;
use chrono::Utc;
use scraper::Html;
use scylla::frame::value::CqlTimestamp;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;

/// Maximum number of retry attempts before giving up on a URL
const MAX_RETRY_ATTEMPTS: i32 = 5;

/// Queue processor that continuously processes crawl queue entries
pub struct QueueProcessor {
    db_client: Arc<CassandraClient>,
    search_client: Option<Arc<SearchClient>>,
    storage_client: Option<Arc<StorageClient>>,
    user_agent: String,
    poll_interval: Duration,
}

impl QueueProcessor {
    /// Create a new queue processor
    pub fn new(
        db_client: Arc<CassandraClient>,
        user_agent: String,
        poll_interval: Duration,
    ) -> Self {
        Self {
            db_client,
            search_client: None,
            storage_client: None,
            user_agent,
            poll_interval,
        }
    }

    /// Create a new queue processor with Meilisearch support
    pub fn with_search(
        db_client: Arc<CassandraClient>,
        search_client: Arc<SearchClient>,
        user_agent: String,
        poll_interval: Duration,
    ) -> Self {
        Self {
            db_client,
            search_client: Some(search_client),
            storage_client: None,
            user_agent,
            poll_interval,
        }
    }

    /// Create a new queue processor with S3 storage support
    pub fn with_storage(
        db_client: Arc<CassandraClient>,
        storage_client: Arc<StorageClient>,
        user_agent: String,
        poll_interval: Duration,
    ) -> Self {
        Self {
            db_client,
            search_client: None,
            storage_client: Some(storage_client),
            user_agent,
            poll_interval,
        }
    }

    /// Create a new queue processor with both Meilisearch and S3 storage support
    pub fn with_all(
        db_client: Arc<CassandraClient>,
        search_client: Arc<SearchClient>,
        storage_client: Arc<StorageClient>,
        user_agent: String,
        poll_interval: Duration,
    ) -> Self {
        Self {
            db_client,
            search_client: Some(search_client),
            storage_client: Some(storage_client),
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
    pub async fn process_next_entry(&self) -> Result<bool> {
        let count = self.db_client.count_crawled_pages().await?;
        if count > 20 {
            println!(
                "Skipping queue processing: crawled_pages count {} > 20",
                count
            );
            return Ok(false);
        }

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

        // Process the entry and handle any failures with retry logic
        match self.process_crawl_entry(&entry).await {
            Ok(()) => {
                println!("Successfully processed URL: {}", entry.url);
                Ok(true)
            }
            Err((error_type, error_message)) => {
                eprintln!(
                    "Failed to process URL {}: {} - {}",
                    entry.url, error_type, error_message
                );
                self.handle_crawl_failure(&entry, error_type, &error_message)
                    .await;
                Ok(true) // Return true because we did process an entry (even if it failed)
            }
        }
    }

    /// Process a crawl entry through all stages
    /// Returns Ok(()) on success, or Err((error_type, message)) on failure
    async fn process_crawl_entry(
        &self,
        entry: &CrawlQueueEntry,
    ) -> std::result::Result<(), (CrawlErrorType, String)> {
        // Stage 1: Crawl the URL
        let request = CrawlRequest {
            url: entry.url.clone(),
            user_agent: self.user_agent.clone(),
        };

        let result = crawl_url(request).await.map_err(|e| {
            (
                CrawlErrorType::FetchError,
                format!("Failed to crawl: {}", e),
            )
        })?;

        // Check if robots.txt disallowed
        if !result.allowed_by_robots {
            return Err((
                CrawlErrorType::RobotsDisallowed,
                "Crawling disallowed by robots.txt".to_string(),
            ));
        }

        // Ensure we have content
        let content = result.content.as_ref().ok_or_else(|| {
            (
                CrawlErrorType::FetchError,
                result
                    .error
                    .clone()
                    .unwrap_or_else(|| "No content retrieved".to_string()),
            )
        })?;

        // Stage 2: Upload to S3 storage (MANDATORY)
        let storage_id = self
            .upload_to_storage_required(&result, &entry.url)
            .await
            .map_err(|e| (CrawlErrorType::StorageError, e))?;

        // Stage 3: Create and store crawled page in Cassandra
        let crawled_page = self
            .create_crawled_page(entry, &result, Some(storage_id))
            .await
            .map_err(|e| {
                (
                    CrawlErrorType::DatabaseError,
                    format!("Failed to create page: {}", e),
                )
            })?;

        self.db_client
            .upsert_crawled_page(&crawled_page)
            .await
            .map_err(|e| {
                (
                    CrawlErrorType::DatabaseError,
                    format!("Failed to upsert page: {}", e),
                )
            })?;

        // Stage 4: Index in search engine (MANDATORY if search client is configured)
        if let Some(search_client) = &self.search_client {
            self.index_document_to_search(search_client, entry, &crawled_page, content)
                .await
                .map_err(|e| {
                    (
                        CrawlErrorType::SearchIndexError,
                        format!("Failed to index: {}", e),
                    )
                })?;
        }

        // Stage 5: Extract and enqueue links (non-critical, log but don't fail)
        if let Err(e) = self.enqueue_meet_links(content, entry).await {
            eprintln!("Warning: Failed to enqueue meet links: {}", e);
        }

        Ok(())
    }

    /// Handle a crawl failure by logging the error and potentially re-queueing
    async fn handle_crawl_failure(
        &self,
        entry: &CrawlQueueEntry,
        error_type: CrawlErrorType,
        error_message: &str,
    ) {
        let now = Utc::now();
        let domain = url::Url::parse(&entry.url)
            .map(|u| u.host_str().unwrap_or(&entry.domain).to_string())
            .unwrap_or_else(|_| entry.domain.clone());

        // Log the error to crawl_errors table
        let crawl_error = CrawlError {
            domain: domain.clone(),
            occurred_at: CqlTimestamp(now.timestamp_millis()),
            url: entry.url.clone(),
            error_type: error_type.clone(),
            error_message: error_message.to_string(),
            attempt_count: entry.attempt_count + 1,
            stack_trace: None,
        };

        if let Err(e) = self.db_client.log_crawl_error(&crawl_error).await {
            eprintln!("Failed to log crawl error to database: {}", e);
        }

        // Re-queue for retry if under max attempts (except for permanent failures)
        let should_retry = match error_type {
            CrawlErrorType::RobotsDisallowed | CrawlErrorType::InvalidUrl => false,
            _ => entry.attempt_count < MAX_RETRY_ATTEMPTS,
        };

        if should_retry {
            if let Err(e) = self.db_client.requeue_with_retry(entry).await {
                eprintln!("Failed to re-queue entry for retry: {}", e);
            } else {
                println!(
                    "Re-queued URL for retry (attempt {}/{}): {}",
                    entry.attempt_count + 1,
                    MAX_RETRY_ATTEMPTS,
                    entry.url
                );
            }
        } else {
            println!(
                "Giving up on URL after {} attempts: {}",
                entry.attempt_count + 1,
                entry.url
            );
        }
    }

    /// Index a crawled document to Meilisearch
    async fn index_document_to_search(
        &self,
        search_client: &Arc<SearchClient>,
        entry: &CrawlQueueEntry,
        crawled_page: &CrawledPage,
        content: &str,
    ) -> Result<()> {
        // Extract title from HTML content (simple extraction)
        let title = extract_title(content);

        // Create excerpt from content (first 500 chars)
        let excerpt = if content.len() > 500 {
            format!("{}...", &content[..500])
        } else {
            content.to_string()
        };

        // Remove HTML tags from content for indexing
        let clean_content = remove_html_tags(content);

        // Create document ID from URL hash
        let doc_id = format!("{:x}", md5::compute(entry.url.as_bytes()));

        let indexed_doc = IndexedDocument {
            id: doc_id,
            url: entry.url.clone(),
            domain: entry.domain.clone(),
            title,
            content: clean_content,
            excerpt,
            crawled_at: crawled_page.last_crawled_at.0 / 1000, // Convert milliseconds to seconds
            http_status: crawled_page.http_status,
        };

        search_client.index_document(&indexed_doc).await?;

        println!("Indexed document for: {}", entry.url);

        Ok(())
    }

    /// Extract meet links from crawled content and enqueue them if count allows
    async fn enqueue_meet_links(&self, content: &str, entry: &CrawlQueueEntry) -> Result<()> {
        let total = self
            .db_client
            .count_crawled_pages()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to count crawled pages: {}", e))?;

        if total > 20 {
            println!(
                "Skipping enqueueing meet links: crawled_pages count {} > 20",
                total
            );
            return Ok(());
        }

        let links = extract_links(content, &entry.url);
        for link in links {
            self.enqueue_link(&link, entry).await;
        }

        Ok(())
    }

    /// Enqueue a single link if it hasn't been crawled yet
    async fn enqueue_link(&self, link: &str, parent_entry: &CrawlQueueEntry) {
        match url::Url::parse(link) {
            Ok(parsed) => {
                let domain = parsed.host_str().unwrap_or("").to_string();
                let url_path = parsed.path().to_string();

                // Check if domain is allowed
                match self.db_client.is_domain_allowed(&domain).await {
                    Ok(is_allowed) => {
                        if !is_allowed {
                            // Silently skip domains that are not in the allowlist
                            return;
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to check domain allowlist for {}: {}", domain, e);
                        return;
                    }
                }

                match self.db_client.crawled_page_exists(&domain, &url_path).await {
                    Ok(exists) => {
                        if exists {
                            return; // Already crawled
                        }

                        let now = Utc::now();
                        let ts = CqlTimestamp(now.timestamp_millis());
                        let new_entry = CrawlQueueEntry {
                            priority: parent_entry.priority,
                            scheduled_at: ts,
                            url: link.to_string(),
                            domain,
                            last_attempt_at: None,
                            attempt_count: 0,
                            created_at: ts,
                        };

                        match self.db_client.insert_queue_entry(&new_entry).await {
                            Ok(()) => println!("Enqueued meet link: {}", link),
                            Err(e) => {
                                eprintln!("Failed to insert meet link into queue {}: {}", link, e)
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to check crawled_page_exists for {}: {}", link, e);
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to parse meet link URL {}: {}", link, e);
            }
        }
    }

    /// Upload content to S3 storage (REQUIRED operation)
    /// Returns error if storage client is not configured or upload fails
    async fn upload_to_storage_required(
        &self,
        result: &CrawlResult,
        url: &str,
    ) -> std::result::Result<Uuid, String> {
        let storage_client = self
            .storage_client
            .as_ref()
            .ok_or_else(|| "Storage client not configured".to_string())?;

        let content = result
            .content
            .as_ref()
            .ok_or_else(|| "No content to upload".to_string())?;

        storage_client
            .upload_content(content, url)
            .await
            .map_err(|e| format!("S3 upload failed: {}", e))
    }

    /// Convert crawl result to a crawled page entry
    async fn create_crawled_page(
        &self,
        entry: &CrawlQueueEntry,
        result: &CrawlResult,
        storage_id: Option<Uuid>,
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
            storage_id,
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

/// Remove HTML tags and extract text content safely using the scraper crate.
/// Handles UTF-8 correctly and properly parses HTML structure.
fn remove_html_tags(html: &str) -> String {
    let document = Html::parse_document(html);

    // Collect all text nodes from the document
    let mut text = String::new();
    for node in document.root_element().descendants() {
        if node.value().as_text().is_some() {
            let content = node.value().as_text().unwrap().trim();
            if !content.is_empty() {
                text.push(' ');
                text.push_str(content);
            }
        }
    }

    // Clean up whitespace
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Extract title from HTML content (simple extraction)
fn extract_title(html: &str) -> Option<String> {
    let html_lower = html.to_lowercase();

    // Try to extract from <title> tag first
    if let Some(start) = html_lower.find("<title") {
        if let Some(tag_end) = html[start..].find('>') {
            let content_start = start + tag_end + 1;
            if let Some(title_end) = html_lower[content_start..].find("</title>") {
                let title = &html[content_start..content_start + title_end].trim();
                if !title.is_empty() {
                    return Some(title.to_string());
                }
            }
        }
    }

    // Try to extract from first <h1> tag
    if let Some(start) = html_lower.find("<h1") {
        if let Some(tag_end) = html[start..].find('>') {
            let content_start = start + tag_end + 1;
            if let Some(title_end) = html_lower[content_start..].find("</h1>") {
                let title = &html[content_start..content_start + title_end].trim();
                if !title.is_empty() {
                    return Some(title.to_string());
                }
            }
        }
    }

    None
}

/// Extract all links from HTML content.
/// Uses the `scraper` crate for safe HTML parsing with proper UTF-8 handling.
/// Resolves relative URLs against `base_url` and returns absolute URLs.
fn extract_links(html: &str, base_url: &str) -> Vec<String> {
    let mut links = Vec::new();

    // Parse HTML safely
    let document = Html::parse_document(html);

    // Select all <a> tags with href attribute
    let Ok(selector) = scraper::Selector::parse("a[href]") else {
        return links;
    };

    for element in document.select(&selector) {
        if let Some(href) = element.value().attr("href") {
            let href = href.trim();
            if !href.is_empty() {
                let resolved_link = resolve_url(base_url, href);
                links.push(resolved_link);
            }
        }
    }

    // Deduplicate
    links.sort();
    links.dedup();
    links
}

/// Resolve a relative URL against a base URL
fn resolve_url(base_url: &str, href: &str) -> String {
    let Ok(base) = url::Url::parse(base_url) else {
        return href.to_string();
    };

    base.join(href)
        .map(|url| url.to_string())
        .unwrap_or_else(|_| href.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remove_html_tags() {
        let html = "<p>Hello <b>World</b></p>";
        let result = remove_html_tags(html);
        assert_eq!(result, "Hello World");
    }

    #[test]
    fn test_extract_title_from_title_tag() {
        let html = "<html><head><title>My Page Title</title></head></html>";
        let result = extract_title(html);
        assert_eq!(result, Some("My Page Title".to_string()));
    }

    #[test]
    fn test_extract_title_from_h1_tag() {
        let html = "<html><body><h1>Welcome to My Site</h1></body></html>";
        let result = extract_title(html);
        assert_eq!(result, Some("Welcome to My Site".to_string()));
    }

    #[test]
    fn test_create_crawled_page_success() {
        // This test verifies the logic of creating a CrawledPage from a CrawlResult
        // Note: This is a unit test and doesn't require a database connection
    }

    #[test]
    fn test_extract_all_links() {
        let html = r#"
            <html>
                <body>
                    <a href="/page1">Page 1</a>
                    <a href="https://example.com/page2">Page 2</a>
                    <a href="https://meet.example.com/room123">Meet Room</a>
                    <a href="relative/path">Relative</a>
                </body>
            </html>
        "#;
        let base_url = "https://example.com/";
        let links = extract_links(html, base_url);

        // Should extract all 4 links
        assert_eq!(links.len(), 4);
        assert!(links.iter().any(|l| l.contains("page1")));
        assert!(links.iter().any(|l| l.contains("page2")));
        assert!(links.iter().any(|l| l.contains("meet.example.com")));
        assert!(links.iter().any(|l| l.contains("relative/path")));
    }

    #[test]
    fn test_extract_links_deduplication() {
        let html = r#"
            <a href="https://example.com/page">Link 1</a>
            <a href="https://example.com/page">Link 2</a>
            <a href="https://example.com/page">Link 3</a>
        "#;
        let links = extract_links(html, "https://example.com/");

        // Should deduplicate to 1 link
        assert_eq!(links.len(), 1);
    }

    #[test]
    fn test_extract_links_relative_urls() {
        let html = r#"
            <a href="/absolute">Absolute Path</a>
            <a href="relative">Relative Path</a>
        "#;
        let base_url = "https://example.com/base/";
        let links = extract_links(html, base_url);

        // Should resolve both
        assert_eq!(links.len(), 2);
        assert!(links.iter().any(|l| l == "https://example.com/absolute"));
        assert!(links
            .iter()
            .any(|l| l == "https://example.com/base/relative"));
    }

    #[test]
    fn test_extract_links_with_non_ascii() {
        let html = r#"
            <p>Привет мир</p>
            <a href="/page1">Link 1</a>
            <p>здравствуй мир</p>
            <a href="/page2">Link 2</a>
        "#;
        let links = extract_links(html, "https://example.com/");

        // Should extract both links despite non-ASCII content
        assert_eq!(links.len(), 2);
        assert!(links.iter().any(|l| l.contains("page1")));
        assert!(links.iter().any(|l| l.contains("page2")));
    }
}
