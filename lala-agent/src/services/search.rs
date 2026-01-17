// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use crate::models::search::{IndexedDocument, SearchRequest, SearchResponse, SearchResult};
use anyhow::Result;
use meilisearch_sdk::client::Client;

/// Meilisearch client wrapper for indexing and searching crawled documents
pub struct SearchClient {
    client: Client,
    index_name: String,
}

impl SearchClient {
    /// Create a new Meilisearch client
    pub async fn new(host: &str, index_name: String) -> Result<Self> {
        // Construct the full URL if only host:port is provided
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
        // Create or get the documents index
        let index = self.client.index(&self.index_name);

        // Set searchable and filterable attributes
        let searchable_attrs = vec!["title", "content", "domain", "url"];
        let _ = index.set_searchable_attributes(searchable_attrs).await;

        // Set filterable attributes for faceted search
        let filterable_attrs = vec!["domain", "http_status", "crawled_at"];
        let _ = index.set_filterable_attributes(filterable_attrs).await;

        // Set sortable attributes
        let sortable_attrs = vec!["crawled_at"];
        let _ = index.set_sortable_attributes(sortable_attrs).await;

        println!("Initialized Meilisearch index: {}", self.index_name);

        Ok(())
    }

    /// Index a single document
    pub async fn index_document(&self, doc: &IndexedDocument) -> Result<()> {
        let index = self.client.index(&self.index_name);

        // Convert document to JSON for indexing
        let doc_json = serde_json::to_value(doc)?;

        // Add or update the document in the index
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

        // Convert documents to JSON
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

    /// Search for documents
    pub async fn search(&self, request: SearchRequest) -> Result<SearchResponse> {
        let index = self.client.index(&self.index_name);

        let limit = request.limit.unwrap_or(20).min(1000) as usize;
        let offset = request.offset.unwrap_or(0) as usize;

        // Perform the search
        let search_result = index
            .search()
            .with_query(&request.query)
            .with_limit(limit)
            .with_offset(offset)
            .execute::<IndexedDocument>()
            .await
            .map_err(|e| anyhow::anyhow!("Search failed: {}", e))?;

        let total = search_result.estimated_total_hits.unwrap_or(0) as u32;

        // Convert results
        let results: Vec<SearchResult> = search_result
            .hits
            .into_iter()
            .map(|hit| SearchResult {
                document: hit.result,
                score: hit.ranking_score.map(|s| s as f32),
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
        let client = SearchClient::new("http://127.0.0.1:7700", "documents".to_string()).await;
        assert!(client.is_ok());
    }

    #[tokio::test]
    #[ignore] // Requires Meilisearch running
    async fn test_index_document() {
        let client = SearchClient::new("http://127.0.0.1:7700", "documents".to_string())
            .await
            .expect("Failed to create client");

        client.init_index().await.expect("Failed to init index");

        let doc = IndexedDocument {
            id: "test-1".to_string(),
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
