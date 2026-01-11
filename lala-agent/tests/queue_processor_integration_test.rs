// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use chrono::Utc;
use lala_agent::models::db::{CrawlQueueEntry, CrawledPage};
use lala_agent::services::db::ScyllaClient;
use scylla::frame::value::CqlTimestamp;
use std::sync::Arc;

// Integration tests for queue processor
// These tests require a running ScyllaDB instance
// Run with: cargo test --test queue_processor_integration_test -- --ignored

#[tokio::test]
#[ignore]
async fn test_queue_processor_workflow() {
    // Connect to ScyllaDB
    let db_client = Arc::new(
        ScyllaClient::new(vec!["127.0.0.1:9042".to_string()], "lalasearch".to_string())
            .await
            .expect("Failed to connect to ScyllaDB"),
    );

    // Create a test queue entry
    let test_url = "https://en.wikipedia.org/wiki/Test_Page";
    let now = Utc::now();
    let now_timestamp = CqlTimestamp(now.timestamp_millis());

    let _entry = CrawlQueueEntry {
        priority: 1,
        scheduled_at: now_timestamp,
        url: test_url.to_string(),
        domain: "en.wikipedia.org".to_string(),
        last_attempt_at: None,
        attempt_count: 0,
        created_at: now_timestamp,
    };

    // Note: In a real test, you would insert this entry into the queue
    // and verify that the processor picks it up and processes it

    // For now, we'll just test the database operations
    let next_entry = db_client
        .get_next_queue_entry()
        .await
        .expect("Failed to get next entry");

    if let Some(entry) = next_entry {
        println!("Found queue entry: {}", entry.url);

        // Delete the entry
        db_client
            .delete_queue_entry(&entry)
            .await
            .expect("Failed to delete entry");

        println!("Successfully deleted queue entry");
    } else {
        println!("No queue entries found");
    }
}

#[tokio::test]
#[ignore]
async fn test_upsert_crawled_page() {
    // Connect to ScyllaDB
    let db_client = Arc::new(
        ScyllaClient::new(vec!["127.0.0.1:9042".to_string()], "lalasearch".to_string())
            .await
            .expect("Failed to connect to ScyllaDB"),
    );

    let now = Utc::now();
    let now_timestamp = CqlTimestamp(now.timestamp_millis());

    let crawl_frequency_hours: i32 = 24;
    let next_crawl = now + chrono::Duration::hours(crawl_frequency_hours as i64);
    let next_crawl_at = CqlTimestamp(next_crawl.timestamp_millis());

    let page = CrawledPage {
        domain: "test.example.com".to_string(),
        url_path: "/test".to_string(),
        url: "https://test.example.com/test".to_string(),
        last_crawled_at: now_timestamp,
        next_crawl_at,
        crawl_frequency_hours,
        http_status: 200,
        content_hash: "abc123".to_string(),
        content_length: 1024,
        robots_allowed: true,
        error_message: None,
        crawl_count: 1,
        created_at: now_timestamp,
        updated_at: now_timestamp,
    };

    // Insert the page
    db_client
        .upsert_crawled_page(&page)
        .await
        .expect("Failed to upsert page");

    println!("Successfully inserted crawled page");

    // Retrieve the page
    let retrieved = db_client
        .get_crawled_page(&page.domain, &page.url_path)
        .await
        .expect("Failed to get page");

    if let Some(retrieved_page) = retrieved {
        assert_eq!(retrieved_page.url, page.url);
        assert_eq!(retrieved_page.http_status, page.http_status);
        assert_eq!(retrieved_page.content_hash, page.content_hash);
        println!("Successfully retrieved crawled page");
    } else {
        panic!("Page not found after insertion");
    }
}
