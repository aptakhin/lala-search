ALTER TABLE crawled_pages
ADD COLUMN IF NOT EXISTS indexed_document_bytes BIGINT;

UPDATE crawled_pages
SET indexed_document_bytes = content_length
WHERE indexed_document_bytes IS NULL
  AND content_length > 0;
