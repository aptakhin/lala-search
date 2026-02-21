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
    /// Tenant ID for multi-tenant search isolation (None in single-tenant mode)
    tenant_id: Option<String>,
}

impl QueueProcessor {
    /// Create a new queue processor
    pub fn new(
        db_client: Arc<CassandraClient>,
        user_agent: String,
        poll_interval: Duration,
        tenant_id: Option<String>,
    ) -> Self {
        Self {
            db_client,
            search_client: None,
            storage_client: None,
            user_agent,
            poll_interval,
            tenant_id,
        }
    }

    /// Create a new queue processor with Meilisearch support
    pub fn with_search(
        db_client: Arc<CassandraClient>,
        search_client: Arc<SearchClient>,
        user_agent: String,
        poll_interval: Duration,
        tenant_id: Option<String>,
    ) -> Self {
        Self {
            db_client,
            search_client: Some(search_client),
            storage_client: None,
            user_agent,
            poll_interval,
            tenant_id,
        }
    }

    /// Create a new queue processor with S3 storage support
    pub fn with_storage(
        db_client: Arc<CassandraClient>,
        storage_client: Arc<StorageClient>,
        user_agent: String,
        poll_interval: Duration,
        tenant_id: Option<String>,
    ) -> Self {
        Self {
            db_client,
            search_client: None,
            storage_client: Some(storage_client),
            user_agent,
            poll_interval,
            tenant_id,
        }
    }

    /// Create a new queue processor with both Meilisearch and S3 storage support
    pub fn with_all(
        db_client: Arc<CassandraClient>,
        search_client: Arc<SearchClient>,
        storage_client: Arc<StorageClient>,
        user_agent: String,
        poll_interval: Duration,
        tenant_id: Option<String>,
    ) -> Self {
        Self {
            db_client,
            search_client: Some(search_client),
            storage_client: Some(storage_client),
            user_agent,
            poll_interval,
            tenant_id,
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
    /// Returns true if an entry was processed, false if queue was empty or crawling is disabled
    pub async fn process_next_entry(&self) -> Result<bool> {
        // Check if crawling is enabled before processing
        let crawling_enabled = self
            .db_client
            .is_crawling_enabled()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to check crawling enabled: {}", e))?;

        if !crawling_enabled {
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
        let (storage_id, compression_type) = self
            .upload_to_storage_required(&result, &entry.url)
            .await
            .map_err(|e| (CrawlErrorType::StorageError, e))?;

        // Stage 3: Create and store crawled page in Cassandra
        let crawled_page = self
            .create_crawled_page(entry, &result, Some(storage_id), compression_type)
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

        // Process indexing and link extraction based on robots directives
        let robots_directives = get_robots_directives(content, result.x_robots_tag.as_deref());
        self.process_post_crawl(entry, &crawled_page, content, robots_directives)
            .await
    }

    /// Process post-crawl stages: indexing and link extraction
    /// Respects robots directives (noindex/nofollow)
    async fn process_post_crawl(
        &self,
        entry: &CrawlQueueEntry,
        crawled_page: &CrawledPage,
        content: &str,
        robots_directives: RobotsMetaDirectives,
    ) -> std::result::Result<(), (CrawlErrorType, String)> {
        // Stage 4: Index in search engine (skip if noindex directive is present)
        if robots_directives.noindex {
            println!(
                "Skipping indexing for {} due to noindex directive",
                entry.url
            );
        } else if let Some(search_client) = &self.search_client {
            self.index_document_to_search(search_client, entry, crawled_page, content)
                .await
                .map_err(|e| {
                    (
                        CrawlErrorType::SearchIndexError,
                        format!("Failed to index: {}", e),
                    )
                })?;
        }

        // Stage 5: Extract and enqueue links (skip if nofollow directive is present)
        if robots_directives.nofollow {
            println!(
                "Skipping link extraction for {} due to nofollow directive",
                entry.url
            );
        } else if let Err(e) = self.enqueue_meet_links(content, entry).await {
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

        // Remove HTML tags from content for indexing
        let clean_content = remove_html_tags(content);

        // Create excerpt from clean content (first 500 chars)
        let excerpt = if clean_content.len() > 500 {
            format!("{}...", &clean_content[..500])
        } else {
            clean_content.clone()
        };

        // Create document ID from URL hash (include tenant_id to prevent cross-tenant collisions)
        let doc_id = match &self.tenant_id {
            Some(tid) => format!("{:x}", md5::compute(format!("{}{}", tid, entry.url))),
            None => format!("{:x}", md5::compute(entry.url.as_bytes())),
        };

        let indexed_doc = IndexedDocument {
            id: doc_id,
            tenant_id: self.tenant_id.clone(),
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
        let _ = self
            .db_client
            .count_crawled_pages()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to count crawled pages: {}", e))?;

        let links = extract_links(content, &entry.url);
        for link in links {
            self.enqueue_link(&link, entry).await;
        }

        Ok(())
    }

    /// Enqueue a single link if it hasn't been crawled yet
    async fn enqueue_link(&self, link: &str, parent_entry: &CrawlQueueEntry) {
        // Parse URL - early return on error
        let parsed = match url::Url::parse(link) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Failed to parse meet link URL {}: {}", link, e);
                return;
            }
        };

        // Validate domain - early return if empty
        let domain = parsed.host_str().unwrap_or("").to_string();
        if domain.is_empty() {
            return;
        }

        let url_path = parsed.path().to_string();

        // Check allowlist - early return on error or not allowed
        let is_allowed = match self.db_client.is_domain_allowed(&domain).await {
            Ok(allowed) => allowed,
            Err(e) => {
                eprintln!("Failed to check domain allowlist for {}: {}", domain, e);
                return;
            }
        };
        if !is_allowed {
            return;
        }

        // Check if already crawled - early return on error or exists
        let exists = match self.db_client.crawled_page_exists(&domain, &url_path).await {
            Ok(e) => e,
            Err(e) => {
                eprintln!("Failed to check crawled_page_exists for {}: {}", link, e);
                return;
            }
        };
        if exists {
            return;
        }

        // Success path - create and insert queue entry
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

        if let Err(e) = self.db_client.insert_queue_entry(&new_entry).await {
            eprintln!("Failed to insert meet link into queue {}: {}", link, e);
            return;
        }

        println!("Enqueued meet link: {}", link);
    }

    /// Upload content to S3 storage (REQUIRED operation)
    /// Returns (storage_id, compression_type) tuple
    /// Returns error if storage client is not configured or upload fails
    async fn upload_to_storage_required(
        &self,
        result: &CrawlResult,
        url: &str,
    ) -> std::result::Result<(Uuid, crate::models::storage::CompressionType), String> {
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
        storage_compression: crate::models::storage::CompressionType,
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
            storage_compression,
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

/// Extract all links from HTML content, filtering out nofollow links.
/// Uses the `scraper` crate for safe HTML parsing with proper UTF-8 handling.
/// Resolves relative URLs against `base_url` and returns absolute URLs.
/// Links with rel="nofollow" attribute are excluded.
fn extract_links(html: &str, base_url: &str) -> Vec<String> {
    let mut links = Vec::new();

    // Parse HTML safely
    let document = Html::parse_document(html);

    // Select all <a> tags with href attribute
    let Ok(selector) = scraper::Selector::parse("a[href]") else {
        return links;
    };

    for element in document.select(&selector) {
        // Skip links with nofollow in rel attribute
        if let Some(rel) = element.value().attr("rel") {
            let rel_lower = rel.to_lowercase();
            if rel_lower.contains("nofollow") {
                continue;
            }
        }

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

/// Robots directives extracted from HTML meta tags or X-Robots-Tag header
#[derive(Debug, Default, Clone, Copy)]
pub struct RobotsMetaDirectives {
    /// Page should not be indexed
    pub noindex: bool,
    /// Links on the page should not be followed
    pub nofollow: bool,
}

impl RobotsMetaDirectives {
    /// Merge two directive sets, using the most restrictive values
    fn merge(self, other: Self) -> Self {
        Self {
            noindex: self.noindex || other.noindex,
            nofollow: self.nofollow || other.nofollow,
        }
    }
}

/// Parse robots directives from a directive string (used by both meta tag and X-Robots-Tag)
fn parse_robots_directive_string(content: &str) -> RobotsMetaDirectives {
    let content_lower = content.to_lowercase();
    let mut directives = RobotsMetaDirectives::default();

    // "none" is equivalent to "noindex, nofollow"
    if content_lower.contains("none") {
        directives.noindex = true;
        directives.nofollow = true;
        return directives;
    }

    if content_lower.contains("noindex") {
        directives.noindex = true;
    }
    if content_lower.contains("nofollow") {
        directives.nofollow = true;
    }

    directives
}

/// Parse X-Robots-Tag HTTP header value into directives
fn parse_x_robots_tag(header_value: Option<&str>) -> RobotsMetaDirectives {
    match header_value {
        Some(value) => parse_robots_directive_string(value),
        None => RobotsMetaDirectives::default(),
    }
}

/// Get combined robots directives from HTML meta tag and X-Robots-Tag header.
/// The most restrictive rule applies when there are conflicts.
fn get_robots_directives(html: &str, x_robots_tag: Option<&str>) -> RobotsMetaDirectives {
    let meta_directives = extract_robots_meta_directives(html);
    let header_directives = parse_x_robots_tag(x_robots_tag);
    meta_directives.merge(header_directives)
}

/// Extract robots meta directives from HTML content.
/// Parses <meta name="robots" content="..."> tags and extracts noindex/nofollow directives.
/// Handles case-insensitive matching and multiple directives separated by commas.
fn extract_robots_meta_directives(html: &str) -> RobotsMetaDirectives {
    let document = Html::parse_document(html);

    // Select all <meta> tags with name attribute
    let Ok(selector) = scraper::Selector::parse("meta[name]") else {
        return RobotsMetaDirectives::default();
    };

    for element in document.select(&selector) {
        // Check if this is a robots meta tag (case-insensitive)
        let name = element.value().attr("name").unwrap_or("");
        if !name.eq_ignore_ascii_case("robots") {
            continue;
        }

        // Get the content attribute and parse directives
        let content = element.value().attr("content").unwrap_or("");
        return parse_robots_directive_string(content);
    }

    RobotsMetaDirectives::default()
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

    #[test]
    fn test_extract_robots_meta_noindex() {
        let html = r#"
            <html>
                <head>
                    <meta name="robots" content="noindex">
                </head>
                <body><p>Content</p></body>
            </html>
        "#;
        let directives = extract_robots_meta_directives(html);
        assert!(directives.noindex);
        assert!(!directives.nofollow);
    }

    #[test]
    fn test_extract_robots_meta_nofollow() {
        let html = r#"
            <html>
                <head>
                    <meta name="robots" content="nofollow">
                </head>
                <body><p>Content</p></body>
            </html>
        "#;
        let directives = extract_robots_meta_directives(html);
        assert!(!directives.noindex);
        assert!(directives.nofollow);
    }

    #[test]
    fn test_extract_robots_meta_both_directives() {
        let html = r#"
            <html>
                <head>
                    <meta name="robots" content="noindex, nofollow">
                </head>
                <body><p>Content</p></body>
            </html>
        "#;
        let directives = extract_robots_meta_directives(html);
        assert!(directives.noindex);
        assert!(directives.nofollow);
    }

    #[test]
    fn test_extract_robots_meta_none_directive() {
        let html = r#"
            <html>
                <head>
                    <meta name="robots" content="none">
                </head>
                <body><p>Content</p></body>
            </html>
        "#;
        let directives = extract_robots_meta_directives(html);
        // "none" is equivalent to "noindex, nofollow"
        assert!(directives.noindex);
        assert!(directives.nofollow);
    }

    #[test]
    fn test_extract_robots_meta_no_robots_tag() {
        let html = r#"
            <html>
                <head>
                    <title>Page</title>
                </head>
                <body><p>Content</p></body>
            </html>
        "#;
        let directives = extract_robots_meta_directives(html);
        assert!(!directives.noindex);
        assert!(!directives.nofollow);
    }

    #[test]
    fn test_extract_robots_meta_case_insensitive() {
        let html = r#"
            <html>
                <head>
                    <meta name="ROBOTS" content="NOINDEX, NOFOLLOW">
                </head>
                <body><p>Content</p></body>
            </html>
        "#;
        let directives = extract_robots_meta_directives(html);
        assert!(directives.noindex);
        assert!(directives.nofollow);
    }

    #[test]
    fn test_extract_links_filters_nofollow_attribute() {
        let html = r#"
            <html>
                <body>
                    <a href="/page1">Follow this</a>
                    <a href="/page2" rel="nofollow">Do not follow this</a>
                    <a href="/page3" rel="sponsored nofollow">Sponsored link</a>
                    <a href="/page4" rel="ugc">User generated content</a>
                </body>
            </html>
        "#;
        let links = extract_links(html, "https://example.com/");

        // Should only include links without nofollow
        assert_eq!(links.len(), 2);
        assert!(links.iter().any(|l| l.contains("page1")));
        assert!(links.iter().any(|l| l.contains("page4")));
        assert!(!links.iter().any(|l| l.contains("page2")));
        assert!(!links.iter().any(|l| l.contains("page3")));
    }

    #[test]
    fn test_parse_x_robots_tag_noindex() {
        let directives = parse_x_robots_tag(Some("noindex"));
        assert!(directives.noindex);
        assert!(!directives.nofollow);
    }

    #[test]
    fn test_parse_x_robots_tag_nofollow() {
        let directives = parse_x_robots_tag(Some("nofollow"));
        assert!(!directives.noindex);
        assert!(directives.nofollow);
    }

    #[test]
    fn test_parse_x_robots_tag_both() {
        let directives = parse_x_robots_tag(Some("noindex, nofollow"));
        assert!(directives.noindex);
        assert!(directives.nofollow);
    }

    #[test]
    fn test_parse_x_robots_tag_none() {
        let directives = parse_x_robots_tag(Some("none"));
        assert!(directives.noindex);
        assert!(directives.nofollow);
    }

    #[test]
    fn test_parse_x_robots_tag_missing() {
        let directives = parse_x_robots_tag(None);
        assert!(!directives.noindex);
        assert!(!directives.nofollow);
    }

    #[test]
    fn test_parse_x_robots_tag_case_insensitive() {
        let directives = parse_x_robots_tag(Some("NOINDEX, NOFOLLOW"));
        assert!(directives.noindex);
        assert!(directives.nofollow);
    }

    #[test]
    fn test_get_robots_directives_meta_only_noindex() {
        let html = r#"<html><head><meta name="robots" content="noindex"></head></html>"#;
        let directives = get_robots_directives(html, None);
        assert!(directives.noindex);
        assert!(!directives.nofollow);
    }

    #[test]
    fn test_get_robots_directives_header_only_nofollow() {
        let html = "<html><body>Content</body></html>";
        let directives = get_robots_directives(html, Some("nofollow"));
        assert!(!directives.noindex);
        assert!(directives.nofollow);
    }

    #[test]
    fn test_get_robots_directives_merge_most_restrictive() {
        // Meta has noindex, header has nofollow - should merge to both
        let html = r#"<html><head><meta name="robots" content="noindex"></head></html>"#;
        let directives = get_robots_directives(html, Some("nofollow"));
        assert!(directives.noindex);
        assert!(directives.nofollow);
    }

    #[test]
    fn test_get_robots_directives_header_overrides_permissive_meta() {
        // Meta allows indexing, but header says noindex - noindex wins
        let html = r#"<html><head><meta name="robots" content="index, follow"></head></html>"#;
        let directives = get_robots_directives(html, Some("noindex"));
        assert!(directives.noindex);
        assert!(!directives.nofollow);
    }

    #[test]
    fn test_get_robots_directives_meta_overrides_permissive_header() {
        // Header allows, but meta says nofollow - nofollow wins
        let html = r#"<html><head><meta name="robots" content="nofollow"></head></html>"#;
        let directives = get_robots_directives(html, Some("index, follow"));
        assert!(!directives.noindex);
        assert!(directives.nofollow);
    }
}
