// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use crate::models::storage::CompressionType;
use anyhow::{anyhow, Result};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use s3::creds::Credentials;
use s3::Bucket;
use s3::Region;
use std::io::{Read, Write};
use uuid::Uuid;

/// Configuration for S3-compatible storage
#[derive(Debug, Clone)]
pub struct S3Config {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key: String,
    pub secret_key: String,
    pub compress_content: bool,
    pub compress_min_size: usize,
}

impl S3Config {
    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self> {
        let endpoint = std::env::var("S3_ENDPOINT")
            .map_err(|_| anyhow!("S3_ENDPOINT environment variable not set"))?;
        let bucket = std::env::var("S3_BUCKET")
            .map_err(|_| anyhow!("S3_BUCKET environment variable not set"))?;
        let access_key = std::env::var("S3_ACCESS_KEY")
            .map_err(|_| anyhow!("S3_ACCESS_KEY environment variable not set"))?;
        let secret_key = std::env::var("S3_SECRET_KEY")
            .map_err(|_| anyhow!("S3_SECRET_KEY environment variable not set"))?;

        let region = std::env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".to_string());
        let compress_content = std::env::var("S3_COMPRESS_CONTENT")
            .unwrap_or_else(|_| "true".to_string())
            .parse()
            .unwrap_or(true);
        let compress_min_size = std::env::var("S3_COMPRESS_MIN_SIZE")
            .unwrap_or_else(|_| "1024".to_string())
            .parse()
            .unwrap_or(1024);

        Ok(Self {
            endpoint,
            region,
            bucket,
            access_key,
            secret_key,
            compress_content,
            compress_min_size,
        })
    }
}

/// S3-compatible storage client for storing crawled HTML content
pub struct StorageClient {
    bucket: Box<Bucket>,
    compress_content: bool,
    compress_min_size: usize,
}

impl StorageClient {
    /// Create a new S3 storage client
    pub async fn new(config: S3Config) -> Result<Self> {
        let region = Region::Custom {
            region: config.region,
            endpoint: config.endpoint,
        };

        let credentials = Credentials::new(
            Some(&config.access_key),
            Some(&config.secret_key),
            None,
            None,
            None,
        )
        .map_err(|e| anyhow!("Failed to create S3 credentials: {}", e))?;

        let bucket = Bucket::new(&config.bucket, region, credentials)
            .map_err(|e| anyhow!("Failed to create S3 bucket: {}", e))?
            .with_path_style();

        println!("Connected to S3 storage bucket: {}", config.bucket);

        Ok(Self {
            bucket,
            compress_content: config.compress_content,
            compress_min_size: config.compress_min_size,
        })
    }

    /// Upload HTML content and return the storage ID (UUID v7) and compression type
    pub async fn upload_content(
        &self,
        content: &str,
        _url: &str,
    ) -> Result<(Uuid, CompressionType)> {
        let storage_id = Uuid::now_v7();

        let (data, compression_type) =
            if self.compress_content && content.len() > self.compress_min_size {
                let compressed = Self::compress(content.as_bytes())?;
                (compressed, CompressionType::Gzip)
            } else {
                (content.as_bytes().to_vec(), CompressionType::None)
            };

        let key = format!("{}.{}", storage_id, compression_type.file_extension());
        let content_type = compression_type.content_type();

        self.bucket
            .put_object_with_content_type(&key, &data, content_type)
            .await
            .map_err(|e| anyhow!("Failed to upload to S3: {}", e))?;

        println!(
            "Uploaded content to S3: {} ({} bytes, {})",
            key,
            data.len(),
            content_type
        );

        Ok((storage_id, compression_type))
    }

    /// Retrieve content by storage ID and compression type
    pub async fn get_content(
        &self,
        storage_id: Uuid,
        compression_type: CompressionType,
    ) -> Result<String> {
        let key = format!("{}.{}", storage_id, compression_type.file_extension());

        let response = self
            .bucket
            .get_object(&key)
            .await
            .map_err(|e| anyhow!("Failed to get object from S3: {}", e))?;

        match compression_type {
            CompressionType::Gzip => {
                let decompressed = Self::decompress(response.bytes())?;
                String::from_utf8(decompressed)
                    .map_err(|e| anyhow!("Invalid UTF-8 in decompressed content: {}", e))
            }
            CompressionType::None => String::from_utf8(response.bytes().to_vec())
                .map_err(|e| anyhow!("Invalid UTF-8 in content: {}", e)),
        }
    }

    /// Compress data using gzip
    fn compress(data: &[u8]) -> Result<Vec<u8>> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(data)
            .map_err(|e| anyhow!("Failed to compress: {}", e))?;
        encoder
            .finish()
            .map_err(|e| anyhow!("Failed to finish compression: {}", e))
    }

    /// Decompress gzip data
    fn decompress(data: &[u8]) -> Result<Vec<u8>> {
        let mut decoder = GzDecoder::new(data);
        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .map_err(|e| anyhow!("Failed to decompress: {}", e))?;
        Ok(decompressed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uuid_v7_is_time_ordered() {
        let id1 = Uuid::now_v7();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let id2 = Uuid::now_v7();

        // UUID v7 should be sortable by time
        assert!(id2 > id1);
    }

    #[test]
    fn test_compress_decompress_roundtrip() {
        let original = "Hello, World! This is a test of gzip compression.";
        let compressed = StorageClient::compress(original.as_bytes()).unwrap();
        let decompressed = StorageClient::decompress(&compressed).unwrap();

        assert_eq!(original.as_bytes(), decompressed.as_slice());
    }

    #[test]
    fn test_compress_reduces_size_for_repetitive_content() {
        let repetitive = "Hello ".repeat(1000);
        let compressed = StorageClient::compress(repetitive.as_bytes()).unwrap();

        // Compressed should be smaller for repetitive content
        assert!(compressed.len() < repetitive.len());
    }

    #[tokio::test]
    #[ignore] // Requires MinIO running
    async fn test_upload_and_retrieve_content() {
        let config = S3Config::from_env().unwrap();
        let client = StorageClient::new(config).await.unwrap();

        let content = "<html><body>Test content</body></html>";
        let url = "https://example.com/test";

        let (storage_id, compression_type) = client.upload_content(content, url).await.unwrap();
        let retrieved = client
            .get_content(storage_id, compression_type)
            .await
            .unwrap();

        assert_eq!(content, retrieved);
    }
}
