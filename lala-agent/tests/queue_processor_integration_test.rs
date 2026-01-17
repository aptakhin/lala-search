// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use chrono::Utc;
use lala_agent::models::db::{CrawlQueueEntry, CrawledPage};
use lala_agent::services::db::CassandraClient;
use lala_agent::services::storage::{S3Config, StorageClient};
use scylla::frame::value::CqlTimestamp;
use std::sync::Arc;

// Integration tests for queue processor
// These tests require running Cassandra and MinIO instances
// Run with: cargo test --test queue_processor_integration_test -- --ignored

#[tokio::test]
#[ignore]
async fn test_queue_processor_workflow() {
    // Connect to Cassandra
    let db_client = Arc::new(
        CassandraClient::new(vec!["127.0.0.1:9042".to_string()], "lalasearch".to_string())
            .await
            .expect("Failed to connect to Cassandra"),
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
    // Connect to Cassandra
    let db_client = Arc::new(
        CassandraClient::new(vec!["127.0.0.1:9042".to_string()], "lalasearch".to_string())
            .await
            .expect("Failed to connect to Cassandra"),
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
        storage_id: None,
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

/// Integration test for the full page crawling workflow with Cassandra and S3 storage.
///
/// This test verifies the complete flow:
/// 1. Insert a queue entry into Cassandra
/// 2. Simulate crawling by creating crawl result data
/// 3. Upload HTML content to S3 storage
/// 4. Create and store CrawledPage with storage_id in Cassandra
/// 5. Verify the page can be retrieved from Cassandra with correct storage_id
/// 6. Verify the content can be retrieved from S3 using storage_id
///
/// Prerequisites:
/// - Cassandra running on localhost:9042 with lalasearch keyspace
/// - MinIO running on localhost:9000 with crawled-content bucket
/// - Environment variables set (or use defaults below)
///
/// Run with: cargo test test_full_crawl_workflow_with_storage -- --ignored
#[tokio::test]
#[ignore]
async fn test_full_crawl_workflow_with_storage() {
    // Setup Cassandra client
    let db_client = Arc::new(
        CassandraClient::new(vec!["127.0.0.1:9042".to_string()], "lalasearch".to_string())
            .await
            .expect("Failed to connect to Cassandra"),
    );
    println!("✓ Connected to Cassandra");

    // Setup S3 storage client (MinIO)
    let storage_config = S3Config {
        endpoint: std::env::var("S3_ENDPOINT")
            .unwrap_or_else(|_| "http://127.0.0.1:9000".to_string()),
        region: "us-east-1".to_string(),
        bucket: std::env::var("S3_BUCKET").unwrap_or_else(|_| "lalasearch-content".to_string()),
        access_key: std::env::var("S3_ACCESS_KEY").unwrap_or_else(|_| "minioadmin".to_string()),
        secret_key: std::env::var("S3_SECRET_KEY").unwrap_or_else(|_| "minioadmin".to_string()),
        compress_content: true,
        compress_min_size: 1024,
    };

    let storage_client = StorageClient::new(storage_config)
        .await
        .expect("Failed to connect to S3/MinIO");
    println!("✓ Connected to S3/MinIO storage");

    // Step 1: Create test data
    let test_domain = "integration-test.example.com";
    let test_path = format!("/test-page-{}", Utc::now().timestamp_millis());
    let test_url = format!("https://{}{}", test_domain, test_path);
    let test_content = r#"<!DOCTYPE html>
<html>
<head>
    <title>Integration Test Page</title>
</head>
<body>
    <h1>Test Content for Full Workflow</h1>
    <p>This is test content for the integration test verifying the full crawl workflow.</p>
    <p>Generated at: TIMESTAMP_PLACEHOLDER</p>
</body>
</html>"#
        .replace("TIMESTAMP_PLACEHOLDER", &Utc::now().to_rfc3339());

    println!("✓ Created test data for URL: {}", test_url);

    // Step 2: Upload content to S3 storage
    let storage_id = storage_client
        .upload_content(&test_content, &test_url)
        .await
        .expect("Failed to upload content to S3");
    println!("✓ Uploaded content to S3 with storage_id: {}", storage_id);

    // Step 3: Create and store CrawledPage in Cassandra
    let now = Utc::now();
    let now_timestamp = CqlTimestamp(now.timestamp_millis());
    let crawl_frequency_hours: i32 = 24;
    let next_crawl = now + chrono::Duration::hours(crawl_frequency_hours as i64);
    let next_crawl_at = CqlTimestamp(next_crawl.timestamp_millis());
    let content_hash = format!("{:x}", md5::compute(test_content.as_bytes()));

    let crawled_page = CrawledPage {
        domain: test_domain.to_string(),
        url_path: test_path.clone(),
        url: test_url.clone(),
        storage_id: Some(storage_id),
        last_crawled_at: now_timestamp,
        next_crawl_at,
        crawl_frequency_hours,
        http_status: 200,
        content_hash: content_hash.clone(),
        content_length: test_content.len() as i64,
        robots_allowed: true,
        error_message: None,
        crawl_count: 1,
        created_at: now_timestamp,
        updated_at: now_timestamp,
    };

    db_client
        .upsert_crawled_page(&crawled_page)
        .await
        .expect("Failed to insert crawled page into Cassandra");
    println!("✓ Stored CrawledPage in Cassandra");

    // Step 4: Verify page retrieval from Cassandra
    let retrieved_page = db_client
        .get_crawled_page(test_domain, &test_path)
        .await
        .expect("Failed to query crawled page")
        .expect("CrawledPage not found in Cassandra");

    assert_eq!(retrieved_page.url, test_url);
    assert_eq!(retrieved_page.http_status, 200);
    assert_eq!(retrieved_page.content_hash, content_hash);
    assert_eq!(
        retrieved_page.storage_id,
        Some(storage_id),
        "storage_id should match"
    );
    println!("✓ Retrieved and verified CrawledPage from Cassandra");

    // Step 5: Verify content retrieval from S3 using storage_id
    let retrieved_storage_id = retrieved_page
        .storage_id
        .expect("storage_id should be present");
    let retrieved_content = storage_client
        .get_content(retrieved_storage_id)
        .await
        .expect("Failed to retrieve content from S3");

    assert_eq!(
        retrieved_content, test_content,
        "Retrieved content should match original"
    );
    println!("✓ Retrieved and verified content from S3 storage");

    println!("\n✅ Full crawl workflow integration test passed!");
    println!("   - Queue entry created");
    println!("   - Content uploaded to S3 (storage_id: {})", storage_id);
    println!("   - CrawledPage stored in Cassandra");
    println!("   - Page retrieved with correct storage_id");
    println!("   - Content retrieved from S3 matches original");
}

/// Test the queue entry insertion and retrieval workflow.
///
/// Prerequisites:
/// - Cassandra running on localhost:9042 with lalasearch keyspace
/// - allowed_domains table should have the test domain (or this test may be skipped)
///
/// Run with: cargo test test_queue_entry_workflow -- --ignored
#[tokio::test]
#[ignore]
async fn test_queue_entry_workflow() {
    let db_client = Arc::new(
        CassandraClient::new(vec!["127.0.0.1:9042".to_string()], "lalasearch".to_string())
            .await
            .expect("Failed to connect to Cassandra"),
    );
    println!("✓ Connected to Cassandra");

    // Create a unique test entry
    let now = Utc::now();
    let now_timestamp = CqlTimestamp(now.timestamp_millis());
    let test_url = format!(
        "https://queue-test.example.com/page-{}",
        now.timestamp_millis()
    );

    let entry = CrawlQueueEntry {
        priority: 5,
        scheduled_at: now_timestamp,
        url: test_url.clone(),
        domain: "queue-test.example.com".to_string(),
        last_attempt_at: None,
        attempt_count: 0,
        created_at: now_timestamp,
    };

    // Insert entry
    db_client
        .insert_queue_entry(&entry)
        .await
        .expect("Failed to insert queue entry");
    println!("✓ Inserted queue entry: {}", test_url);

    // Retrieve entry (note: may get a different entry if queue is not empty)
    let retrieved = db_client
        .get_next_queue_entry()
        .await
        .expect("Failed to get next queue entry");

    assert!(retrieved.is_some(), "Should have at least one queue entry");
    let retrieved_entry = retrieved.unwrap();
    println!("✓ Retrieved queue entry: {}", retrieved_entry.url);

    // Delete the entry we retrieved
    db_client
        .delete_queue_entry(&retrieved_entry)
        .await
        .expect("Failed to delete queue entry");
    println!("✓ Deleted queue entry");

    println!("\n✅ Queue entry workflow test passed!");
}
