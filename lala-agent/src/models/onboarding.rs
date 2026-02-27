// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use serde::{Deserialize, Serialize};

/// Query parameters for the recent crawled pages endpoint.
#[derive(Debug, Deserialize)]
pub struct RecentPagesQuery {
    /// Domain to filter results by
    pub domain: String,
    /// Max results to return (default: 10)
    pub limit: Option<u32>,
    /// When true, enrich results with titles/excerpts from Meilisearch
    pub enrich: Option<bool>,
}

/// A single recently crawled page summary for the onboarding console view.
#[derive(Debug, Serialize)]
pub struct RecentPageInfo {
    pub url: String,
    pub http_status: i32,
    pub content_length: i32,
    /// Unix timestamp (seconds since epoch)
    pub last_crawled_at: i64,
    /// Page title from Meilisearch (only populated when enrich=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Page excerpt from Meilisearch (only populated when enrich=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
}

/// Response for the recent crawled pages endpoint.
#[derive(Debug, Serialize)]
pub struct RecentPagesResponse {
    pub pages: Vec<RecentPageInfo>,
    pub total: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recent_pages_response_serialization() {
        let response = RecentPagesResponse {
            pages: vec![RecentPageInfo {
                url: "https://example.com/page".to_string(),
                http_status: 200,
                content_length: 15432,
                last_crawled_at: 1772150400,
                title: None,
                excerpt: None,
            }],
            total: 1,
        };
        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["total"], 1);
        assert_eq!(json["pages"][0]["url"], "https://example.com/page");
        assert_eq!(json["pages"][0]["http_status"], 200);
        assert_eq!(json["pages"][0]["content_length"], 15432);
        assert_eq!(json["pages"][0]["last_crawled_at"], 1772150400);
        // title and excerpt should be omitted when None
        assert!(json["pages"][0].get("title").is_none());
        assert!(json["pages"][0].get("excerpt").is_none());
    }

    #[test]
    fn test_recent_page_info_with_enrichment() {
        let page = RecentPageInfo {
            url: "https://example.com/about".to_string(),
            http_status: 200,
            content_length: 8921,
            last_crawled_at: 1772150400,
            title: Some("About Us".to_string()),
            excerpt: Some("We are a technology company...".to_string()),
        };
        let json = serde_json::to_value(&page).unwrap();
        assert_eq!(json["title"], "About Us");
        assert_eq!(json["excerpt"], "We are a technology company...");
    }

    #[test]
    fn test_recent_pages_query_deserialization() {
        let json = r#"{"domain":"example.com","limit":20,"enrich":true}"#;
        let query: RecentPagesQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.domain, "example.com");
        assert_eq!(query.limit, Some(20));
        assert_eq!(query.enrich, Some(true));
    }

    #[test]
    fn test_recent_pages_query_defaults() {
        let json = r#"{"domain":"example.com"}"#;
        let query: RecentPagesQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.domain, "example.com");
        assert_eq!(query.limit, None);
        assert_eq!(query.enrich, None);
    }
}
