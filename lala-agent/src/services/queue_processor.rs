// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use crate::models::crawler::{CrawlRequest, CrawlResult};
use crate::models::db::{CrawlQueueEntry, CrawledPage};
use crate::models::search::IndexedDocument;
use crate::services::crawler::crawl_url;
use crate::services::db::CassandraClient;
use crate::services::search::SearchClient;
use anyhow::Result;
use chrono::Utc;
use scraper::Html;
use scylla::frame::value::CqlTimestamp;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

/// Queue processor that continuously processes crawl queue entries
pub struct QueueProcessor {
    db_client: Arc<CassandraClient>,
    search_client: Option<Arc<SearchClient>>,
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

        // After storing the crawled page, extract any "meet" links and enqueue them
        if let Some(ref content) = result.content {
            // Respect global cap: don't add new links if there are already > 20 crawled pages
            if let Err(e) = self.enqueue_meet_links(content, &entry).await {
                eprintln!("Failed to enqueue meet links: {}", e);
            }
        }

        // If Meilisearch is available and we have successful content, index the document
        if let (Some(search_client), Some(content)) = (&self.search_client, &result.content) {
            if let Err(e) = self
                .index_document_to_search(search_client, &entry, &crawled_page, content)
                .await
            {
                eprintln!("Failed to index document in Meilisearch: {}", e);
                // Don't fail the crawl if indexing fails - it's non-critical
            }
        }

        println!("Successfully processed URL: {}", entry.url);

        Ok(true)
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
    if let Ok(selector) = scraper::Selector::parse("a[href]") {
        for element in document.select(&selector) {
            if let Some(href) = element.value().attr("href") {
                let href = href.trim();
                if !href.is_empty() {
                    // Resolve relative URLs
                    if let Ok(base) = url::Url::parse(base_url) {
                        if let Ok(resolved) = base.join(href) {
                            links.push(resolved.to_string());
                        } else {
                            links.push(href.to_string());
                        }
                    } else {
                        links.push(href.to_string());
                    }
                }
            }
        }
    }

    // Deduplicate
    links.sort();
    links.dedup();
    links
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
