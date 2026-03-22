// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use crate::models::crawler::{CrawlRequest, CrawlResult};
use chrono::{DateTime, Utc};
use reqwest::header::{HeaderMap, CACHE_CONTROL, EXPIRES};
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant};
use texting_robots::{get_robots_url, Robot};

const DEFAULT_ROBOTS_CACHE_TTL: Duration = Duration::from_secs(30 * 60);
const MAX_ROBOTS_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

struct RobotsCacheEntry {
    body: String,
    expires_at: Instant,
}

fn robots_cache() -> &'static RwLock<HashMap<String, RobotsCacheEntry>> {
    static ROBOTS_CACHE: OnceLock<RwLock<HashMap<String, RobotsCacheEntry>>> = OnceLock::new();
    ROBOTS_CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

fn clamp_robots_cache_ttl(ttl: Duration) -> Duration {
    ttl.clamp(DEFAULT_ROBOTS_CACHE_TTL, MAX_ROBOTS_CACHE_TTL)
}

fn robots_cache_ttl_from_headers(headers: &HeaderMap, now: DateTime<Utc>) -> Duration {
    if let Some(ttl) = max_age_ttl(headers) {
        return clamp_robots_cache_ttl(ttl);
    }

    if let Some(ttl) = expires_ttl(headers, now) {
        return clamp_robots_cache_ttl(ttl);
    }

    DEFAULT_ROBOTS_CACHE_TTL
}

fn max_age_ttl(headers: &HeaderMap) -> Option<Duration> {
    let cache_control = headers.get(CACHE_CONTROL)?.to_str().ok()?;

    for directive in cache_control.split(',') {
        let Some((key, value)) = directive.trim().split_once('=') else {
            continue;
        };
        if !key.eq_ignore_ascii_case("max-age") {
            continue;
        }

        let seconds = value.trim().trim_matches('"').parse::<u64>().ok()?;
        return Some(Duration::from_secs(seconds));
    }

    None
}

fn expires_ttl(headers: &HeaderMap, now: DateTime<Utc>) -> Option<Duration> {
    let expires = headers.get(EXPIRES)?.to_str().ok()?;
    let expires_at = DateTime::parse_from_rfc2822(expires).ok()?;
    let ttl = expires_at.with_timezone(&Utc).signed_duration_since(now);
    Some(Duration::from_secs(ttl.num_seconds().max(0) as u64))
}

fn cached_robots_txt(robots_url: &str, now: Instant) -> Option<String> {
    let cache = robots_cache().read().ok()?;
    let entry = cache.get(robots_url)?;
    (entry.expires_at > now).then(|| entry.body.clone())
}

fn cache_robots_txt(robots_url: &str, body: String, ttl: Duration, now: Instant) {
    if let Ok(mut cache) = robots_cache().write() {
        cache.insert(
            robots_url.to_string(),
            RobotsCacheEntry {
                body,
                expires_at: now + ttl,
            },
        );
    }
}

async fn get_robots_txt(client: &reqwest::Client, robots_url: &str) -> String {
    let now = Instant::now();
    if let Some(body) = cached_robots_txt(robots_url, now) {
        return body;
    }

    let response = match client.get(robots_url).send().await {
        Ok(response) => response,
        Err(_) => {
            // If robots.txt doesn't exist or can't be fetched, assume allowed.
            let body = String::new();
            cache_robots_txt(robots_url, body.clone(), DEFAULT_ROBOTS_CACHE_TTL, now);
            return body;
        }
    };

    let ttl = robots_cache_ttl_from_headers(response.headers(), Utc::now());
    let body = response.text().await.unwrap_or_default();
    cache_robots_txt(robots_url, body.clone(), ttl, now);
    body
}

/// Crawl a URL, respecting robots.txt rules
pub async fn crawl_url(request: CrawlRequest) -> Result<CrawlResult, Box<dyn std::error::Error>> {
    if let Err(e) = url::Url::parse(&request.url) {
        return Ok(CrawlResult {
            url: request.url,
            allowed_by_robots: false,
            content: None,
            error: Some(format!("Invalid URL: {}", e)),
            x_robots_tag: None,
        });
    }

    let robots_url = get_robots_url(&request.url)?;

    let client = reqwest::Client::new();
    let robots_txt = get_robots_txt(&client, &robots_url).await;

    let robot = Robot::new(&request.user_agent, robots_txt.as_bytes())?;

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
    use reqwest::header::{HeaderMap, HeaderValue};

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

    #[test]
    fn test_robots_cache_ttl_uses_default_when_headers_missing() {
        let headers = HeaderMap::new();

        assert_eq!(
            robots_cache_ttl_from_headers(&headers, Utc::now()),
            DEFAULT_ROBOTS_CACHE_TTL
        );
    }

    #[test]
    fn test_robots_cache_ttl_never_refreshes_faster_than_default() {
        let mut headers = HeaderMap::new();
        headers.insert(CACHE_CONTROL, HeaderValue::from_static("max-age=60"));

        assert_eq!(
            robots_cache_ttl_from_headers(&headers, Utc::now()),
            DEFAULT_ROBOTS_CACHE_TTL
        );
    }

    #[test]
    fn test_robots_cache_ttl_honors_longer_max_age_up_to_cap() {
        let mut headers = HeaderMap::new();
        headers.insert(
            CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=7200"),
        );

        assert_eq!(
            robots_cache_ttl_from_headers(&headers, Utc::now()),
            Duration::from_secs(7200)
        );
    }

    #[test]
    fn test_robots_cache_ttl_caps_max_age_at_twenty_four_hours() {
        let mut headers = HeaderMap::new();
        headers.insert(CACHE_CONTROL, HeaderValue::from_static("max-age=999999"));

        assert_eq!(
            robots_cache_ttl_from_headers(&headers, Utc::now()),
            MAX_ROBOTS_CACHE_TTL
        );
    }
}
