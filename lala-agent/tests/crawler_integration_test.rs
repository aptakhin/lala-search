// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use lala_agent::models::crawler::CrawlRequest;
use lala_agent::services::crawler::crawl_url;

#[tokio::test]
async fn test_crawl_wikipedia_main_page() {
    let request = CrawlRequest {
        url: "https://www.wikipedia.org/".to_string(),
        user_agent: "LalaSearchBot/0.1 (Educational Purpose)".to_string(),
    };

    let result = crawl_url(request).await;
    assert!(result.is_ok(), "Failed to crawl Wikipedia: {:?}", result);

    let crawl_result = result.unwrap();
    assert_eq!(crawl_result.url, "https://www.wikipedia.org/");
    assert!(
        crawl_result.allowed_by_robots,
        "Wikipedia main page should be allowed by robots.txt"
    );
    assert!(
        crawl_result.content.is_some(),
        "Should have content when allowed"
    );
    assert!(crawl_result.error.is_none(), "Should not have error");

    let content = crawl_result.content.unwrap();
    assert!(!content.is_empty(), "Content should not be empty");

    // Verify it's HTML content
    assert!(
        content.contains("<html") || content.contains("<!DOCTYPE"),
        "Content should be HTML"
    );

    // Wikipedia main page typically contains certain elements
    assert!(
        content.to_lowercase().contains("wikipedia"),
        "Content should mention Wikipedia"
    );

    println!("Successfully crawled Wikipedia main page!");
    println!("Content length: {} bytes", content.len());
    println!("First 200 chars: {}", &content[..content.len().min(200)]);
}
