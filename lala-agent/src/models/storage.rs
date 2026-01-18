// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

/// Compression type for stored content
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionType {
    /// No compression applied
    None,
    /// Gzip compression
    Gzip,
}

impl CompressionType {
    /// Get the file extension for this compression type
    pub fn file_extension(&self) -> &str {
        match self {
            CompressionType::None => "html",
            CompressionType::Gzip => "html.gz",
        }
    }

    /// Get the content type header for S3
    pub fn content_type(&self) -> &str {
        match self {
            CompressionType::None => "text/html",
            CompressionType::Gzip => "application/gzip",
        }
    }

    /// Convert to database tinyint representation
    pub fn to_db_value(&self) -> i8 {
        match self {
            CompressionType::None => 0,
            CompressionType::Gzip => 1,
        }
    }

    /// Parse from database tinyint representation
    pub fn from_db_value(value: i8) -> Self {
        match value {
            1 => CompressionType::Gzip,
            _ => CompressionType::None,
        }
    }

    /// Convert to database string representation (for display)
    pub fn to_db_string(&self) -> &str {
        match self {
            CompressionType::None => "none",
            CompressionType::Gzip => "gzip",
        }
    }
}

impl std::fmt::Display for CompressionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_db_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compression_type_file_extension() {
        assert_eq!(CompressionType::None.file_extension(), "html");
        assert_eq!(CompressionType::Gzip.file_extension(), "html.gz");
    }

    #[test]
    fn test_compression_type_content_type() {
        assert_eq!(CompressionType::None.content_type(), "text/html");
        assert_eq!(CompressionType::Gzip.content_type(), "application/gzip");
    }

    #[test]
    fn test_compression_type_db_value_roundtrip() {
        let types = vec![CompressionType::None, CompressionType::Gzip];

        for compression_type in types {
            let db_value = compression_type.to_db_value();
            let parsed = CompressionType::from_db_value(db_value);
            assert_eq!(compression_type, parsed);
        }
    }

    #[test]
    fn test_compression_type_from_db_value_invalid() {
        assert_eq!(CompressionType::from_db_value(99), CompressionType::None);
        assert_eq!(CompressionType::from_db_value(-1), CompressionType::None);
    }

    #[test]
    fn test_compression_type_db_values() {
        assert_eq!(CompressionType::None.to_db_value(), 0);
        assert_eq!(CompressionType::Gzip.to_db_value(), 1);
    }

    #[test]
    fn test_compression_type_display() {
        assert_eq!(format!("{}", CompressionType::None), "none");
        assert_eq!(format!("{}", CompressionType::Gzip), "gzip");
    }
}
