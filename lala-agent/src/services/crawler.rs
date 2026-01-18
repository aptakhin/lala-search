// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use crate::models::crawler::{CrawlRequest, CrawlResult};
use texting_robots::{get_robots_url, Robot};

/// Crawl a URL, respecting robots.txt rules
pub async fn crawl_url(request: CrawlRequest) -> Result<CrawlResult, Box<dyn std::error::Error>> {
    // Validate the URL
    if let Err(e) = url::Url::parse(&request.url) {
        return Ok(CrawlResult {
            url: request.url,
            allowed_by_robots: false,
            content: None,
            error: Some(format!("Invalid URL: {}", e)),
            x_robots_tag: None,
        });
    }

    // Get robots.txt URL
    let robots_url = get_robots_url(&request.url)?;

    // Fetch and parse robots.txt
    let client = reqwest::Client::new();
    let robots_txt = match client.get(&robots_url).send().await {
        Ok(response) => response.text().await.unwrap_or_default(),
        Err(_) => {
            // If robots.txt doesn't exist or can't be fetched, assume allowed
            String::new()
        }
    };

    // Parse robots.txt
    let robot = Robot::new(&request.user_agent, robots_txt.as_bytes())?;

    // Check if crawling is allowed
    let allowed = robot.allowed(&request.url);

    if !allowed {
        return Ok(CrawlResult {
            url: request.url,
            allowed_by_robots: false,
            content: None,
            error: None,
            x_robots_tag: None,
        });
    }

    // Fetch the content
    let response = client
        .get(&request.url)
        .header("User-Agent", &request.user_agent)
        .send()
        .await;

    match response {
        Ok(resp) => {
            // Extract X-Robots-Tag header before consuming response
            let x_robots_tag = resp
                .headers()
                .get("x-robots-tag")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());

            let content = resp.text().await?;
            Ok(CrawlResult {
                url: request.url,
                allowed_by_robots: true,
                content: Some(content),
                error: None,
                x_robots_tag,
            })
        }
        Err(e) => Ok(CrawlResult {
            url: request.url,
            allowed_by_robots: true,
            content: None,
            error: Some(format!("Failed to fetch content: {}", e)),
            x_robots_tag: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_crawl_checks_robots_txt_allowed() {
        // Test that crawling checks robots.txt and allows when permitted
        let request = CrawlRequest {
            url: "https://en.wikipedia.org/wiki/Main_Page".to_string(),
            user_agent: "LalaSearchBot/0.1".to_string(),
        };

        let result = crawl_url(request).await;
        assert!(result.is_ok());

        let crawl_result = result.unwrap();
        assert_eq!(crawl_result.url, "https://en.wikipedia.org/wiki/Main_Page");
        // Wikipedia typically allows crawling of /wiki/ paths
        assert!(crawl_result.allowed_by_robots);
    }

    #[tokio::test]
    async fn test_crawl_checks_robots_txt_disallowed() {
        // Test that crawling respects robots.txt disallow rules
        let request = CrawlRequest {
            url: "https://www.google.com/search".to_string(),
            user_agent: "LalaSearchBot/0.1".to_string(),
        };

        let result = crawl_url(request).await;
        assert!(result.is_ok());

        let crawl_result = result.unwrap();
        assert_eq!(crawl_result.url, "https://www.google.com/search");
        // Google typically disallows crawling of /search
        assert!(!crawl_result.allowed_by_robots);
        assert!(crawl_result.content.is_none());
    }

    #[tokio::test]
    async fn test_crawl_returns_content_when_allowed() {
        // Test that content is returned when robots.txt allows
        let request = CrawlRequest {
            url: "https://en.wikipedia.org/wiki/Main_Page".to_string(),
            user_agent: "LalaSearchBot/0.1".to_string(),
        };

        let result = crawl_url(request).await;
        assert!(result.is_ok());

        let crawl_result = result.unwrap();
        assert!(crawl_result.allowed_by_robots);
        assert!(crawl_result.content.is_some());
        assert!(crawl_result.error.is_none());

        let content = crawl_result.content.unwrap();
        assert!(!content.is_empty());
        // Wikipedia pages should contain HTML
        assert!(content.contains("<html") || content.contains("<!DOCTYPE"));
    }

    #[tokio::test]
    async fn test_crawl_handles_invalid_url() {
        // Test that invalid URLs are handled gracefully
        let request = CrawlRequest {
            url: "not-a-valid-url".to_string(),
            user_agent: "LalaSearchBot/0.1".to_string(),
        };

        let result = crawl_url(request).await;
        // Should return an error or a result with error field set
        assert!(result.is_err() || result.unwrap().error.is_some());
    }

    #[tokio::test]
    async fn test_crawl_homepage_allowed() {
        // Test that homepage is typically allowed
        let request = CrawlRequest {
            url: "https://en.wikipedia.org/".to_string(),
            user_agent: "LalaSearchBot/0.1".to_string(),
        };

        let result = crawl_url(request).await;
        assert!(result.is_ok());

        let crawl_result = result.unwrap();
        // Most sites allow crawling of homepage
        assert!(crawl_result.allowed_by_robots);
    }
}
