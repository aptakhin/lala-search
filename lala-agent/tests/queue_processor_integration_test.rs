// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use axum::{routing::get, Router};
use chrono::Utc;
use lala_agent::models::db::{CrawlQueueEntry, CrawledPage};
use lala_agent::services::db::{CassandraClient, CassandraConfig};
use lala_agent::services::queue_processor::QueueProcessor;
use lala_agent::services::storage::{S3Config, StorageClient};
use scylla::frame::value::CqlTimestamp;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

// Integration tests for queue processor workflows.
// These tests require running Cassandra and MinIO instances.
// Run with: cargo test --test queue_processor_integration_test -- --ignored
//
// Required environment variables:
// - CASSANDRA_HOSTS: Cassandra host(s), e.g., "127.0.0.1:9042"
// - CASSANDRA_KEYSPACE: Cassandra keyspace, e.g., "lalasearch"
// - S3_ENDPOINT: MinIO/S3 endpoint, e.g., "http://127.0.0.1:9000"
// - S3_BUCKET: S3 bucket name
// - S3_ACCESS_KEY: S3 access key
// - S3_SECRET_KEY: S3 secret key

/// Helper to create a CassandraClient from environment variables.
async fn create_db_client() -> Arc<CassandraClient> {
    let config = CassandraConfig::from_env()
        .expect("CASSANDRA_HOSTS and CASSANDRA_KEYSPACE environment variables must be set");
    Arc::new(
        CassandraClient::from_config(config)
            .await
            .expect("Failed to connect to Cassandra"),
    )
}

/// Helper to create a StorageClient from environment variables.
async fn create_storage_client() -> StorageClient {
    let config = S3Config::from_env().expect(
        "S3_ENDPOINT, S3_BUCKET, S3_ACCESS_KEY, S3_SECRET_KEY environment variables must be set",
    );
    StorageClient::new(config)
        .await
        .expect("Failed to connect to S3/MinIO")
}

#[tokio::test]
#[ignore]
async fn test_queue_processor_workflow() {
    let db_client = create_db_client().await;
    println!("✓ Connected to Cassandra");

    // Setup: Create a unique test domain and ensure it's allowed
    let test_domain = format!(
        "queue-test-{}.example.invalid",
        Utc::now().timestamp_millis()
    );

    db_client
        .insert_allowed_domain(&test_domain, "test", Some("Test domain"))
        .await
        .expect("Failed to insert test domain into allowed_domains");
    println!("✓ Set up allowed domain: {}", test_domain);

    // Create a test queue entry
    let test_url = format!("https://{}/test-page", test_domain);
    let now = Utc::now();
    let now_timestamp = CqlTimestamp(now.timestamp_millis());

    let entry = CrawlQueueEntry {
        priority: 1,
        scheduled_at: now_timestamp,
        url: test_url.clone(),
        domain: test_domain.clone(),
        last_attempt_at: None,
        attempt_count: 0,
        created_at: now_timestamp,
    };

    // Insert the queue entry
    db_client
        .insert_queue_entry(&entry)
        .await
        .expect("Failed to insert queue entry");
    println!("✓ Inserted queue entry: {}", test_url);

    // Retrieve the next entry - should get our entry
    let next_entry = db_client
        .get_next_queue_entry()
        .await
        .expect("Failed to get next entry");

    assert!(next_entry.is_some(), "Should have a queue entry");
    let retrieved_entry = next_entry.unwrap();
    println!("✓ Retrieved queue entry: {}", retrieved_entry.url);

    // Delete the entry
    db_client
        .delete_queue_entry(&retrieved_entry)
        .await
        .expect("Failed to delete entry");
    println!("✓ Deleted queue entry");

    // Cleanup: Remove the test domain
    db_client
        .delete_allowed_domain(&test_domain)
        .await
        .expect("Failed to clean up test domain");
    println!("✓ Cleaned up test domain");

    println!("\n✅ Queue processor workflow test passed!");
}

#[tokio::test]
#[ignore]
async fn test_upsert_crawled_page() {
    let db_client = create_db_client().await;
    println!("✓ Connected to Cassandra");

    // Use unique test data to ensure isolation
    let test_domain = format!(
        "crawled-page-test-{}.example.invalid",
        Utc::now().timestamp_millis()
    );
    let test_path = "/test-page";
    let test_url = format!("https://{}{}", test_domain, test_path);

    let now = Utc::now();
    let now_timestamp = CqlTimestamp(now.timestamp_millis());
    let crawl_frequency_hours: i32 = 24;
    let next_crawl = now + chrono::Duration::hours(crawl_frequency_hours as i64);
    let next_crawl_at = CqlTimestamp(next_crawl.timestamp_millis());

    let page = CrawledPage {
        domain: test_domain.clone(),
        url_path: test_path.to_string(),
        url: test_url.clone(),
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
    println!("✓ Inserted crawled page: {}", test_url);

    // Retrieve the page
    let retrieved = db_client
        .get_crawled_page(&test_domain, test_path)
        .await
        .expect("Failed to get page");

    assert!(retrieved.is_some(), "Page should exist after insertion");
    let retrieved_page = retrieved.unwrap();

    assert_eq!(retrieved_page.url, page.url);
    assert_eq!(retrieved_page.http_status, page.http_status);
    assert_eq!(retrieved_page.content_hash, page.content_hash);
    println!("✓ Retrieved and verified crawled page");

    // Cleanup: Delete the test page
    db_client
        .delete_crawled_page(&test_domain, test_path)
        .await
        .expect("Failed to clean up test page");
    println!("✓ Cleaned up test page");

    println!("\n✅ Upsert crawled page test passed!");
}

/// Integration test for the full page crawling workflow with Cassandra and S3 storage.
///
/// This test verifies the complete flow:
/// 1. Set up test domain in allowed_domains
/// 2. Upload HTML content to S3 storage
/// 3. Create and store CrawledPage with storage_id in Cassandra
/// 4. Verify the page can be retrieved from Cassandra with correct storage_id
/// 5. Verify the content can be retrieved from S3 using storage_id
/// 6. Clean up all test data
#[tokio::test]
#[ignore]
async fn test_full_crawl_workflow_with_storage() {
    // Setup clients from environment variables
    let db_client = create_db_client().await;
    println!("✓ Connected to Cassandra");

    let storage_client = create_storage_client().await;
    println!("✓ Connected to S3/MinIO storage");

    // Step 1: Create unique test data and set up allowed domain
    let test_domain = format!(
        "full-workflow-test-{}.example.invalid",
        Utc::now().timestamp_millis()
    );
    let test_path = "/test-page";
    let test_url = format!("https://{}{}", test_domain, test_path);
    let test_content = format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>Integration Test Page</title>
</head>
<body>
    <h1>Test Content for Full Workflow</h1>
    <p>This is test content for the integration test verifying the full crawl workflow.</p>
    <p>Generated at: {}</p>
</body>
</html>"#,
        Utc::now().to_rfc3339()
    );

    // Set up allowed domain
    db_client
        .insert_allowed_domain(&test_domain, "test", Some("Test domain"))
        .await
        .expect("Failed to insert test domain into allowed_domains");
    println!("✓ Set up allowed domain: {}", test_domain);
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
        domain: test_domain.clone(),
        url_path: test_path.to_string(),
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
        .get_crawled_page(&test_domain, test_path)
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

    // Step 6: Cleanup
    db_client
        .delete_crawled_page(&test_domain, test_path)
        .await
        .expect("Failed to clean up test page");
    db_client
        .delete_allowed_domain(&test_domain)
        .await
        .expect("Failed to clean up test domain");
    println!("✓ Cleaned up test data");

    println!("\n✅ Full crawl workflow integration test passed!");
    println!("   - Allowed domain set up");
    println!("   - Content uploaded to S3 (storage_id: {})", storage_id);
    println!("   - CrawledPage stored in Cassandra");
    println!("   - Page retrieved with correct storage_id");
    println!("   - Content retrieved from S3 matches original");
    println!("   - Test data cleaned up");
}

/// Test the queue entry insertion and retrieval workflow with proper isolation.
#[tokio::test]
#[ignore]
async fn test_queue_entry_workflow() {
    let db_client = create_db_client().await;
    println!("✓ Connected to Cassandra");

    // Setup: Create a unique test domain and ensure it's allowed
    let test_domain = format!(
        "queue-entry-test-{}.example.invalid",
        Utc::now().timestamp_millis()
    );

    db_client
        .insert_allowed_domain(&test_domain, "test", Some("Test domain"))
        .await
        .expect("Failed to insert test domain into allowed_domains");
    println!("✓ Set up allowed domain: {}", test_domain);

    // Create a unique test entry
    let now = Utc::now();
    let now_timestamp = CqlTimestamp(now.timestamp_millis());
    let test_url = format!("https://{}/page-{}", test_domain, now.timestamp_millis());

    let entry = CrawlQueueEntry {
        priority: 5,
        scheduled_at: now_timestamp,
        url: test_url.clone(),
        domain: test_domain.clone(),
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

    // Retrieve entry
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

    // Cleanup: Remove the test domain
    db_client
        .delete_allowed_domain(&test_domain)
        .await
        .expect("Failed to clean up test domain");
    println!("✓ Cleaned up test domain");

    println!("\n✅ Queue entry workflow test passed!");
}

/// Integration test for the complete crawl pipeline using production code.
///
/// This test verifies the full end-to-end flow with a single production code call:
/// 1. Setup: Start local HTTP server, add allowed domain
/// 2. Add page to crawl queue
/// 3. Call QueueProcessor::process_next_entry() (single production code call)
/// 4. Verify: crawled page in Cassandra + content in S3
/// 5. Cleanup
#[tokio::test]
#[ignore]
async fn test_crawl_pipeline_end_to_end() {
    // Test content that will be served by our local HTTP server
    let test_html = r#"<!DOCTYPE html>
<html>
<head>
    <title>Integration Test Page</title>
</head>
<body>
    <h1>Test Content</h1>
    <p>This is test content for the end-to-end integration test.</p>
</body>
</html>"#;

    // robots.txt that allows all crawling
    let robots_txt = "User-agent: *\nAllow: /\n";

    // Start local HTTP server
    let test_html_clone = test_html.to_string();
    let robots_txt_clone = robots_txt.to_string();

    let app = Router::new()
        .route(
            "/test-page",
            get(move || {
                let html = test_html_clone.clone();
                async move { html }
            }),
        )
        .route(
            "/robots.txt",
            get(move || {
                let robots = robots_txt_clone.clone();
                async move { robots }
            }),
        );

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind test server");
    let port = listener.local_addr().unwrap().port();
    let test_domain = format!("127.0.0.1:{}", port);
    let test_url = format!("http://{}/test-page", test_domain);

    // Start server in background
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    println!("✓ Started local HTTP server on port {}", port);

    // Setup clients
    let db_client = create_db_client().await;
    println!("✓ Connected to Cassandra");

    let storage_client = create_storage_client().await;
    println!("✓ Connected to S3/MinIO storage");

    // Step 1: Setup - add allowed domain
    db_client
        .insert_allowed_domain(&test_domain, "test", Some("Test domain"))
        .await
        .expect("Failed to insert allowed domain");
    println!("✓ Added allowed domain: {}", test_domain);

    // Step 2: Add page to crawl queue
    let now = Utc::now();
    let now_timestamp = CqlTimestamp(now.timestamp_millis());

    let queue_entry = CrawlQueueEntry {
        priority: 1,
        scheduled_at: now_timestamp,
        url: test_url.clone(),
        domain: test_domain.clone(),
        last_attempt_at: None,
        attempt_count: 0,
        created_at: now_timestamp,
    };

    db_client
        .insert_queue_entry(&queue_entry)
        .await
        .expect("Failed to insert queue entry");
    println!("✓ Added queue entry: {}", test_url);

    // Step 3: Process crawl using production code (SINGLE CALL)
    let processor = QueueProcessor::with_storage(
        db_client.clone(),
        Arc::new(storage_client),
        "LalaSearchBot/0.1 (Integration Test)".to_string(),
        Duration::from_secs(1),
    );

    let processed = processor
        .process_next_entry()
        .await
        .expect("process_next_entry failed");

    assert!(processed, "Should have processed the queue entry");
    println!("✓ Processed crawl entry via production code");

    // Step 4: Verify results
    // 4a: Check crawled page exists in Cassandra
    let crawled_page = db_client
        .get_crawled_page(&test_domain, "/test-page")
        .await
        .expect("Failed to query crawled page")
        .expect("Crawled page not found in Cassandra");

    assert_eq!(crawled_page.url, test_url);
    assert_eq!(crawled_page.http_status, 200);
    assert!(crawled_page.robots_allowed);
    assert!(
        crawled_page.storage_id.is_some(),
        "storage_id should be set"
    );
    println!("✓ Verified crawled page in Cassandra");

    // 4b: Check content exists in S3 and matches
    let storage_client = create_storage_client().await;
    let storage_id = crawled_page.storage_id.unwrap();
    let retrieved_content = storage_client
        .get_content(storage_id)
        .await
        .expect("Failed to retrieve content from S3");

    assert_eq!(retrieved_content, test_html, "S3 content should match");
    println!("✓ Verified content in S3 matches original");

    // Step 5: Cleanup
    db_client
        .delete_crawled_page(&test_domain, "/test-page")
        .await
        .expect("Failed to cleanup crawled page");
    db_client
        .delete_allowed_domain(&test_domain)
        .await
        .expect("Failed to cleanup allowed domain");
    println!("✓ Cleaned up test data");

    // Abort the server
    server_handle.abort();

    println!("\n✅ End-to-end crawl pipeline test passed!");
    println!("   - Local HTTP server served test content");
    println!("   - Single process_next_entry() call processed the crawl");
    println!("   - Crawled page verified in Cassandra");
    println!("   - Content verified in S3");
}
