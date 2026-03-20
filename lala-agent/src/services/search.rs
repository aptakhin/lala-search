// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use crate::models::search::{IndexedDocument, SearchRequest, SearchResponse, SearchResult};
use anyhow::Result;
use meilisearch_sdk::client::Client;
use meilisearch_sdk::search::Selectors;

/// Meilisearch client wrapper for indexing and searching crawled documents
pub struct SearchClient {
    client: Client,
    index_name: String,
}

impl SearchClient {
    /// Create a new Meilisearch client
    pub async fn new(host: &str, index_name: String) -> Result<Self> {
        let url = if host.starts_with("http://") || host.starts_with("https://") {
            host.to_string()
        } else {
            format!("http://{}", host)
        };

        let client = Client::new(&url, None::<String>)?;

        println!("Connected to Meilisearch at {}", url);

        Ok(Self { client, index_name })
    }

    /// Initialize the documents index with proper settings
    pub async fn init_index(&self) -> Result<()> {
        let index = self.client.index(&self.index_name);

        let searchable_attrs = vec!["title", "content", "domain", "url"];
        let _ = index.set_searchable_attributes(searchable_attrs).await;

        let filterable_attrs = vec!["domain", "crawled_at", "tenant_id"];
        let _ = index.set_filterable_attributes(filterable_attrs).await;

        let sortable_attrs = vec!["crawled_at"];
        let _ = index.set_sortable_attributes(sortable_attrs).await;

        println!("Initialized Meilisearch index: {}", self.index_name);

        Ok(())
    }

    /// Index a single document
    pub async fn index_document(&self, doc: &IndexedDocument) -> Result<()> {
        let index = self.client.index(&self.index_name);

        let doc_json = serde_json::to_value(doc)?;

        index
            .add_documents(&[doc_json], Some("id"))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to index document: {}", e))?;

        Ok(())
    }

    /// Index multiple documents in batch
    pub async fn index_documents(&self, docs: &[IndexedDocument]) -> Result<()> {
        if docs.is_empty() {
            return Ok(());
        }

        let index = self.client.index(&self.index_name);

        let doc_jsons: Vec<_> = docs
            .iter()
            .filter_map(|doc| serde_json::to_value(doc).ok())
            .collect();

        if !doc_jsons.is_empty() {
            index
                .add_documents(&doc_jsons, Some("id"))
                .await
                .map_err(|e| anyhow::anyhow!("Failed to batch index documents: {}", e))?;

            println!("Indexed {} documents", doc_jsons.len());
        }

        Ok(())
    }

    /// Search for documents, optionally filtering by tenant_id (multi-tenant mode)
    pub async fn search(
        &self,
        request: SearchRequest,
        tenant_id: Option<&str>,
    ) -> Result<SearchResponse> {
        let index = self.client.index(&self.index_name);

        let limit = request.limit.unwrap_or(20).min(1000) as usize;
        let offset = request.offset.unwrap_or(0) as usize;

        let tenant_filter = tenant_id.map(|tid| format!("tenant_id = '{}'", tid));

        let mut query = index.search();
        query
            .with_query(&request.query)
            .with_limit(limit)
            .with_offset(offset)
            .with_attributes_to_crop(Selectors::Some(&[("content", Some(200))]))
            .with_crop_length(200)
            .with_attributes_to_highlight(Selectors::Some(&["content"]))
            .with_highlight_pre_tag("<mark>")
            .with_highlight_post_tag("</mark>");

        if let Some(ref filter) = tenant_filter {
            query.with_filter(filter);
        }

        let search_result = query
            .execute::<IndexedDocument>()
            .await
            .map_err(|e| anyhow::anyhow!("Search failed: {}", e))?;

        let total = search_result.estimated_total_hits.unwrap_or(0) as u32;

        let results: Vec<SearchResult> = search_result
            .hits
            .into_iter()
            .map(|hit| {
                let snippet = hit
                    .formatted_result
                    .as_ref()
                    .and_then(|formatted| formatted.get("content"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                SearchResult {
                    document: hit.result,
                    score: hit.ranking_score.map(|s| s as f32),
                    snippet,
                }
            })
            .collect();

        Ok(SearchResponse {
            results,
            total,
            processing_ms: 0, // Meilisearch SDK doesn't expose processing time in the response
        })
    }

    /// Delete a document from the index
    pub async fn delete_document(&self, doc_id: &str) -> Result<()> {
        let index = self.client.index(&self.index_name);

        index
            .delete_document(doc_id)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to delete document: {}", e))?;

        Ok(())
    }

    /// Clear all documents from the index
    pub async fn clear_index(&self) -> Result<()> {
        let index = self.client.index(&self.index_name);

        index
            .delete_all_documents()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to clear index: {}", e))?;

        println!("Cleared all documents from index");

        Ok(())
    }

    /// List documents by domain, sorted by crawled_at descending.
    /// Uses an empty-query search with domain filter for browsing.
    pub async fn list_by_domain(
        &self,
        domain: &str,
        tenant_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<IndexedDocument>> {
        let index = self.client.index(&self.index_name);

        let filter = match tenant_id {
            Some(tid) => format!("domain = '{}' AND tenant_id = '{}'", domain, tid),
            None => format!("domain = '{}'", domain),
        };

        let mut query = index.search();
        query
            .with_query("")
            .with_limit(limit)
            .with_filter(&filter)
            .with_sort(&["crawled_at:desc"]);

        let search_result = query
            .execute::<IndexedDocument>()
            .await
            .map_err(|e| anyhow::anyhow!("List by domain failed (domain={}): {}", domain, e))?;

        let docs: Vec<IndexedDocument> = search_result
            .hits
            .into_iter()
            .map(|hit| hit.result)
            .collect();

        Ok(docs)
    }

    /// Get index statistics
    pub async fn get_stats(&self) -> Result<String> {
        let _stats = self.client.get_stats().await?;
        // Return a simple status message since the SDK doesn't have a serializable stats response
        Ok("Meilisearch is operational".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires Meilisearch running
    async fn test_search_client_creation() {
        let host = std::env::var("MEILISEARCH_HOST")
            .expect("MEILISEARCH_HOST environment variable must be set");
        let client = SearchClient::new(&host, "documents".to_string()).await;
        assert!(client.is_ok());
    }

    #[tokio::test]
    #[ignore] // Requires Meilisearch running
    async fn test_list_by_domain_returns_documents_sorted_by_crawled_at() {
        let host = std::env::var("MEILISEARCH_HOST")
            .expect("MEILISEARCH_HOST environment variable must be set");
        let index_name = format!(
            "test_list_by_domain_{}",
            chrono::Utc::now().timestamp_millis()
        );
        let client = SearchClient::new(&host, index_name)
            .await
            .expect("Failed to create client");
        client.init_index().await.expect("Failed to init index");

        let docs = vec![
            IndexedDocument {
                id: "lbd-old".to_string(),
                tenant_id: None,
                url: "https://example.com/old".to_string(),
                domain: "example.com".to_string(),
                title: Some("Old Page".to_string()),
                content: "Old content".to_string(),
                excerpt: "Old content".to_string(),
                crawled_at: 1000000000,
                http_status: 200,
            },
            IndexedDocument {
                id: "lbd-new".to_string(),
                tenant_id: None,
                url: "https://example.com/new".to_string(),
                domain: "example.com".to_string(),
                title: Some("New Page".to_string()),
                content: "New content".to_string(),
                excerpt: "New content".to_string(),
                crawled_at: 1700000000,
                http_status: 200,
            },
        ];
        client
            .index_documents(&docs)
            .await
            .expect("Failed to index");

        // Wait for Meilisearch to process
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let results = client
            .list_by_domain("example.com", None, 10)
            .await
            .expect("Failed to list by domain");

        assert!(results.len() >= 2);
        // First result should be the newer document (sorted by crawled_at desc)
        assert_eq!(results[0].id, "lbd-new");

        // Cleanup
        client.clear_index().await.expect("Failed to clear");
    }

    #[tokio::test]
    #[ignore] // Requires Meilisearch running
    async fn test_index_document() {
        let host = std::env::var("MEILISEARCH_HOST")
            .expect("MEILISEARCH_HOST environment variable must be set");
        let client = SearchClient::new(&host, "documents".to_string())
            .await
            .expect("Failed to create client");

        client.init_index().await.expect("Failed to init index");

        let doc = IndexedDocument {
            id: "test-1".to_string(),
            tenant_id: None,
            url: "https://example.com/page".to_string(),
            domain: "example.com".to_string(),
            title: Some("Example Page".to_string()),
            content: "This is example content for testing".to_string(),
            excerpt: "This is example content for testing".to_string(),
            crawled_at: 1234567890,
            http_status: 200,
        };

        let result = client.index_document(&doc).await;
        assert!(result.is_ok());
    }
}
