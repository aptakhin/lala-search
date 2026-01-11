// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use serde::{Deserialize, Serialize};

/// Document to be indexed in Meilisearch for full-text search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedDocument {
    /// Unique document ID (typically url_hash)
    pub id: String,
    /// The URL of the document
    pub url: String,
    /// The domain the document was crawled from
    pub domain: String,
    /// The title/heading extracted from the document
    pub title: Option<String>,
    /// Text content extracted from the document (searchable)
    pub content: String,
    /// First 500 characters of content for preview
    pub excerpt: String,
    /// Timestamp when the document was crawled (seconds since epoch)
    pub crawled_at: i64,
    /// HTTP status code of the response
    pub http_status: i32,
}

/// Request to search indexed documents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    /// Search query string
    pub query: String,
    /// Maximum number of results to return (default: 20)
    pub limit: Option<u32>,
    /// Offset for pagination (default: 0)
    pub offset: Option<u32>,
}

/// Search result from Meilisearch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// The indexed document
    pub document: IndexedDocument,
    /// Relevance score
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
}

/// Search response containing results and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    /// Search results
    pub results: Vec<SearchResult>,
    /// Total number of matching documents
    pub total: u32,
    /// Processing time in milliseconds
    pub processing_ms: u32,
}
