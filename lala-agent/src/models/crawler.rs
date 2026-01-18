// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use serde::{Deserialize, Serialize};

/// Request to crawl a specific URL
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlRequest {
    /// The URL to crawl
    pub url: String,
    /// User agent string to use for the request
    pub user_agent: String,
}

/// Result of a crawl operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlResult {
    /// The URL that was crawled
    pub url: String,
    /// Whether the crawl was allowed by robots.txt
    pub allowed_by_robots: bool,
    /// The raw HTML content (if crawl was allowed and successful)
    pub content: Option<String>,
    /// Any error message if the crawl failed
    pub error: Option<String>,
    /// X-Robots-Tag HTTP header value (if present in response)
    pub x_robots_tag: Option<String>,
}
