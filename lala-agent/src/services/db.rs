// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use crate::models::action_history::{ActionRecord, ActionType, EntityType};
use crate::models::db::{CrawlError, CrawlQueueEntry, CrawledPage};
use crate::models::storage::CompressionType;
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use sqlx::postgres::PgPool;
use sqlx::Row;
use uuid::Uuid;

const INDEX_CAPACITY_SETTING_KEY: &str = "index_capacity_bytes";

/// PostgreSQL database client for managing crawl queue and crawled pages.
///
/// Each client is scoped to a `tenant_id`. All tenant-specific queries filter by this ID.
/// Call `with_tenant()` to create a client scoped to a different tenant.
#[derive(Clone)]
pub struct DbClient {
    pool: PgPool,
    pub tenant_id: Uuid,
}

impl DbClient {
    /// Create a new database client scoped to a tenant.
    pub fn new(pool: PgPool, tenant_id: Uuid) -> Self {
        Self { pool, tenant_id }
    }

    /// Get a reference to the underlying connection pool.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Create a new client targeting a different tenant, sharing the same connection pool.
    pub fn with_tenant(&self, tenant_id: Uuid) -> Self {
        Self {
            pool: self.pool.clone(),
            tenant_id,
        }
    }

    /// Ensure the default tenant row exists.
    /// Creates the tenant with the given name if missing; never overwrites an existing name
    /// (use `PUT /admin/settings/tenant-name` to rename).
    pub async fn ensure_default_tenant(&self, tenant_id: Uuid, name: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO tenants (tenant_id, name) VALUES ($1, $2) \
             ON CONFLICT (tenant_id) DO NOTHING",
        )
        .bind(tenant_id)
        .bind(name)
        .execute(&self.pool)
        .await
        .context("Failed to ensure default tenant")?;
        Ok(())
    }

    /// Update the display name of the current tenant.
    pub async fn update_tenant_name(&self, name: &str) -> Result<()> {
        sqlx::query("UPDATE tenants SET name = $1 WHERE tenant_id = $2 AND deleted_at IS NULL")
            .bind(name)
            .bind(self.tenant_id)
            .execute(&self.pool)
            .await
            .context("Failed to update tenant name")?;
        Ok(())
    }

    /// Get the display name of the current tenant.
    pub async fn get_tenant_name(&self) -> Result<String> {
        let row: (String,) =
            sqlx::query_as("SELECT name FROM tenants WHERE tenant_id = $1 AND deleted_at IS NULL")
                .bind(self.tenant_id)
                .fetch_one(&self.pool)
                .await
                .context("Failed to get tenant name")?;
        Ok(row.0)
    }

    /// List all active tenant IDs from the tenants table.
    pub async fn list_tenant_ids(&self) -> Result<Vec<Uuid>> {
        let rows = sqlx::query("SELECT tenant_id FROM tenants WHERE deleted_at IS NULL")
            .fetch_all(&self.pool)
            .await
            .context("Failed to list tenant IDs")?;

        Ok(rows.iter().map(|r| r.get("tenant_id")).collect())
    }

    /// Insert an allowed domain
    pub async fn insert_allowed_domain(
        &self,
        domain: &str,
        added_by: &str,
        notes: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO allowed_domains (tenant_id, domain, added_by, notes)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (tenant_id, domain) DO UPDATE SET deleted_at = NULL, added_by = $3, notes = $4, added_at = now()",
        )
        .bind(self.tenant_id)
        .bind(domain)
        .bind(added_by)
        .bind(notes)
        .execute(&self.pool)
        .await
        .with_context(|| format!("Failed to insert allowed domain: {domain}"))?;
        Ok(())
    }

    /// Soft-delete an allowed domain
    pub async fn delete_allowed_domain(&self, domain: &str) -> Result<()> {
        sqlx::query(
            "UPDATE allowed_domains SET deleted_at = now() WHERE tenant_id = $1 AND domain = $2",
        )
        .bind(self.tenant_id)
        .bind(domain)
        .execute(&self.pool)
        .await
        .with_context(|| format!("Failed to soft-delete allowed domain: {domain}"))?;
        Ok(())
    }

    /// Hard-delete an allowed domain (for test cleanup only)
    pub async fn hard_delete_allowed_domain(&self, domain: &str) -> Result<()> {
        sqlx::query("DELETE FROM allowed_domains WHERE tenant_id = $1 AND domain = $2")
            .bind(self.tenant_id)
            .bind(domain)
            .execute(&self.pool)
            .await
            .with_context(|| format!("Failed to hard-delete allowed domain: {domain}"))?;
        Ok(())
    }

    /// List all active allowed domains
    pub async fn list_allowed_domains(
        &self,
    ) -> Result<Vec<(String, Option<String>, Option<String>, Option<String>)>> {
        let rows = sqlx::query(
            "SELECT domain, added_by, notes, added_at
             FROM allowed_domains
             WHERE tenant_id = $1 AND deleted_at IS NULL",
        )
        .bind(self.tenant_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to list allowed domains")?;

        Ok(rows
            .iter()
            .map(|r| {
                let domain: String = r.get("domain");
                let added_by: Option<String> = r.get("added_by");
                let notes: Option<String> = r.get("notes");
                let added_at: Option<DateTime<Utc>> = r.get("added_at");
                let added_at_str = added_at.map(|dt| dt.to_rfc3339());
                (domain, added_by, notes, added_at_str)
            })
            .collect())
    }

    /// Delete a crawled page by domain and url_path (hard delete)
    pub async fn delete_crawled_page(&self, domain: &str, url_path: &str) -> Result<()> {
        sqlx::query(
            "DELETE FROM crawled_pages WHERE tenant_id = $1 AND domain = $2 AND url_path = $3",
        )
        .bind(self.tenant_id)
        .bind(domain)
        .bind(url_path)
        .execute(&self.pool)
        .await
        .with_context(|| format!("Failed to delete crawled page: {domain}{url_path}"))?;
        Ok(())
    }

    /// Get the next entry from the crawl queue using FOR UPDATE SKIP LOCKED.
    /// Returns None if the queue is empty.
    pub async fn get_next_queue_entry(&self) -> Result<Option<CrawlQueueEntry>> {
        let row = sqlx::query(
            "SELECT queue_id, tenant_id, priority, scheduled_at, url, domain,
                    last_attempt_at, attempt_count, created_at
             FROM crawl_queue
             WHERE tenant_id = $1 AND scheduled_at <= now()
             ORDER BY priority, scheduled_at
             LIMIT 1
             FOR UPDATE SKIP LOCKED",
        )
        .bind(self.tenant_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get next queue entry")?;

        Ok(row.map(|r| CrawlQueueEntry {
            queue_id: r.get("queue_id"),
            tenant_id: r.get("tenant_id"),
            priority: r.get("priority"),
            scheduled_at: r.get("scheduled_at"),
            url: r.get("url"),
            domain: r.get("domain"),
            last_attempt_at: r.get("last_attempt_at"),
            attempt_count: r.get("attempt_count"),
            created_at: r.get("created_at"),
        }))
    }

    /// Check if a crawled page exists by domain + url_path
    pub async fn crawled_page_exists(&self, domain: &str, url_path: &str) -> Result<bool> {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM crawled_pages WHERE tenant_id = $1 AND domain = $2 AND url_path = $3)",
        )
        .bind(self.tenant_id)
        .bind(domain)
        .bind(url_path)
        .fetch_one(&self.pool)
        .await
        .with_context(|| format!("Failed to check crawled page exists: {domain}{url_path}"))?;

        Ok(exists)
    }

    /// Insert an entry into the crawl queue. Ignores duplicates (same tenant + url).
    pub async fn insert_queue_entry(&self, entry: &CrawlQueueEntry) -> Result<()> {
        sqlx::query(
            "INSERT INTO crawl_queue (queue_id, tenant_id, priority, scheduled_at, url, domain,
                                      last_attempt_at, attempt_count, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (tenant_id, url) DO NOTHING",
        )
        .bind(entry.queue_id)
        .bind(entry.tenant_id)
        .bind(entry.priority)
        .bind(entry.scheduled_at)
        .bind(&entry.url)
        .bind(&entry.domain)
        .bind(entry.last_attempt_at)
        .bind(entry.attempt_count)
        .bind(entry.created_at)
        .execute(&self.pool)
        .await
        .with_context(|| format!("Failed to insert queue entry: {}", entry.url))?;
        Ok(())
    }

    /// Delete an entry from the crawl queue by queue_id
    pub async fn delete_queue_entry(&self, entry: &CrawlQueueEntry) -> Result<()> {
        sqlx::query("DELETE FROM crawl_queue WHERE queue_id = $1")
            .bind(entry.queue_id)
            .execute(&self.pool)
            .await
            .with_context(|| format!("Failed to delete queue entry: {}", entry.url))?;
        Ok(())
    }

    /// Insert or update a crawled page
    pub async fn upsert_crawled_page(&self, page: &CrawledPage) -> Result<()> {
        sqlx::query(
            "INSERT INTO crawled_pages
             (page_id, tenant_id, domain, url_path, url, storage_id, storage_compression,
              last_crawled_at, next_crawl_at, crawl_frequency_hours, http_status,
              content_hash, content_length, indexed_document_bytes, robots_allowed,
              error_message, crawl_count, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)
             ON CONFLICT (tenant_id, domain, url_path) DO UPDATE SET
                storage_id = EXCLUDED.storage_id,
                storage_compression = EXCLUDED.storage_compression,
                last_crawled_at = EXCLUDED.last_crawled_at,
                next_crawl_at = EXCLUDED.next_crawl_at,
                crawl_frequency_hours = EXCLUDED.crawl_frequency_hours,
                http_status = EXCLUDED.http_status,
                content_hash = EXCLUDED.content_hash,
                content_length = EXCLUDED.content_length,
                indexed_document_bytes = EXCLUDED.indexed_document_bytes,
                robots_allowed = EXCLUDED.robots_allowed,
                error_message = EXCLUDED.error_message,
                crawl_count = EXCLUDED.crawl_count,
                updated_at = EXCLUDED.updated_at",
        )
        .bind(page.page_id)
        .bind(page.tenant_id)
        .bind(&page.domain)
        .bind(&page.url_path)
        .bind(&page.url)
        .bind(page.storage_id)
        .bind(page.storage_compression.to_db_value())
        .bind(page.last_crawled_at)
        .bind(page.next_crawl_at)
        .bind(page.crawl_frequency_hours)
        .bind(page.http_status)
        .bind(&page.content_hash)
        .bind(page.content_length)
        .bind(page.indexed_document_bytes)
        .bind(page.robots_allowed)
        .bind(page.error_message.as_deref())
        .bind(page.crawl_count)
        .bind(page.created_at)
        .bind(page.updated_at)
        .execute(&self.pool)
        .await
        .with_context(|| format!("Failed to upsert crawled page: {}{}", page.domain, page.url_path))?;
        Ok(())
    }

    /// Check if a domain is in the active allowed domains list
    pub async fn is_domain_allowed(&self, domain: &str) -> Result<bool> {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM allowed_domains WHERE tenant_id = $1 AND domain = $2 AND deleted_at IS NULL)",
        )
        .bind(self.tenant_id)
        .bind(domain)
        .fetch_one(&self.pool)
        .await
        .with_context(|| format!("Failed to check if domain is allowed: {domain}"))?;

        Ok(exists)
    }

    /// Get a crawled page by domain and url_path
    pub async fn get_crawled_page(
        &self,
        domain: &str,
        url_path: &str,
    ) -> Result<Option<CrawledPage>> {
        let row = sqlx::query(
            "SELECT page_id, tenant_id, domain, url_path, url, storage_id, storage_compression,
                    last_crawled_at, next_crawl_at, crawl_frequency_hours, http_status,
                    content_hash, content_length, indexed_document_bytes, robots_allowed,
                    error_message, crawl_count, created_at, updated_at
             FROM crawled_pages
             WHERE tenant_id = $1 AND domain = $2 AND url_path = $3",
        )
        .bind(self.tenant_id)
        .bind(domain)
        .bind(url_path)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("Failed to get crawled page: {domain}{url_path}"))?;

        Ok(row.map(|r| {
            let compression_value: Option<i16> = r.get("storage_compression");
            CrawledPage {
                page_id: r.get("page_id"),
                tenant_id: r.get("tenant_id"),
                domain: r.get("domain"),
                url_path: r.get("url_path"),
                url: r.get("url"),
                storage_id: r.get("storage_id"),
                storage_compression: CompressionType::from_db_value(compression_value),
                last_crawled_at: r.get("last_crawled_at"),
                next_crawl_at: r.get("next_crawl_at"),
                crawl_frequency_hours: r.get("crawl_frequency_hours"),
                http_status: r.get("http_status"),
                content_hash: r.get("content_hash"),
                content_length: r.get("content_length"),
                indexed_document_bytes: r.get("indexed_document_bytes"),
                robots_allowed: r.get("robots_allowed"),
                error_message: r.get("error_message"),
                crawl_count: r.get("crawl_count"),
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
            }
        }))
    }

    /// Log a crawl error to the crawl_errors table
    pub async fn log_crawl_error(&self, error: &CrawlError) -> Result<()> {
        sqlx::query(
            "INSERT INTO crawl_errors
             (error_id, tenant_id, page_id, domain, url, error_type, error_message,
              attempt_count, stack_trace, occurred_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(error.error_id)
        .bind(error.tenant_id)
        .bind(error.page_id)
        .bind(&error.domain)
        .bind(&error.url)
        .bind(error.error_type.to_string())
        .bind(&error.error_message)
        .bind(error.attempt_count)
        .bind(error.stack_trace.as_deref())
        .bind(error.occurred_at)
        .execute(&self.pool)
        .await
        .with_context(|| format!("Failed to log crawl error for: {}", error.url))?;
        Ok(())
    }

    // ========== Settings Methods ==========

    /// Get a setting value by key
    pub async fn get_setting(&self, key: &str) -> Result<Option<String>> {
        sqlx::query_scalar::<_, Option<String>>(
            "SELECT setting_value FROM settings WHERE tenant_id = $1 AND setting_key = $2",
        )
        .bind(self.tenant_id)
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("Failed to get setting: {key}"))
        .map(|value| value.flatten())
    }

    /// Set a setting value by key (upsert)
    pub async fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO settings (tenant_id, setting_key, setting_value, updated_at)
             VALUES ($1, $2, $3, now())
             ON CONFLICT (tenant_id, setting_key) DO UPDATE SET setting_value = $3, updated_at = now()",
        )
        .bind(self.tenant_id)
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await
        .with_context(|| format!("Failed to set setting: {key}"))?;
        Ok(())
    }

    /// Check if crawling is enabled.
    /// Returns the value from settings table, or defaults based on ENVIRONMENT:
    /// - dev: defaults to true (enabled)
    /// - prod: defaults to false (disabled for safety)
    pub async fn is_crawling_enabled(&self) -> Result<bool> {
        match self.get_setting("crawling_enabled").await? {
            Some(value) => Ok(value == "true"),
            None => {
                let is_dev = std::env::var("ENVIRONMENT")
                    .map(|v| v == "dev")
                    .unwrap_or(false);
                Ok(is_dev)
            }
        }
    }

    /// Set crawling enabled/disabled
    pub async fn set_crawling_enabled(&self, enabled: bool) -> Result<()> {
        let value = if enabled { "true" } else { "false" };
        self.set_setting("crawling_enabled", value).await
    }

    /// Get the indexed document usage for the current tenant.
    pub async fn get_index_usage_bytes(&self) -> Result<i64> {
        sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(SUM(indexed_document_bytes), 0)::BIGINT
             FROM crawled_pages
             WHERE tenant_id = $1 AND indexed_document_bytes IS NOT NULL",
        )
        .bind(self.tenant_id)
        .fetch_one(&self.pool)
        .await
        .context("Failed to get indexed document usage")
    }

    /// Get the maximum indexed document capacity for the current tenant.
    pub async fn get_index_capacity_bytes(&self) -> Result<i64> {
        match self.get_setting(INDEX_CAPACITY_SETTING_KEY).await? {
            Some(value) => value.parse().with_context(|| {
                format!(
                    "Failed to parse tenant setting {}={} as i64",
                    INDEX_CAPACITY_SETTING_KEY, value
                )
            }),
            None => Ok(std::env::var("TENANT_INDEX_CAPACITY_BYTES")
                .context("TENANT_INDEX_CAPACITY_BYTES must be set")?
                .parse()
                .context("TENANT_INDEX_CAPACITY_BYTES must be a valid number")?),
        }
    }

    /// Store the maximum indexed document capacity for the current tenant.
    pub async fn set_index_capacity_bytes(&self, max_bytes: i64) -> Result<()> {
        if max_bytes <= 0 {
            bail!("index capacity must be greater than zero");
        }

        self.set_setting(INDEX_CAPACITY_SETTING_KEY, &max_bytes.to_string())
            .await
    }

    /// Get recently crawled pages for a domain, ordered by last_crawled_at descending.
    /// Returns tuples of (url, http_status, content_length, last_crawled_at).
    pub async fn get_recent_crawled_pages(
        &self,
        domain: &str,
        limit: i64,
    ) -> Result<Vec<(String, i32, i32, DateTime<Utc>)>> {
        let rows = sqlx::query(
            "SELECT url, http_status, content_length, last_crawled_at
             FROM crawled_pages
             WHERE tenant_id = $1 AND domain = $2
             ORDER BY last_crawled_at DESC
             LIMIT $3",
        )
        .bind(self.tenant_id)
        .bind(domain)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .with_context(|| format!("Failed to get recent crawled pages for domain: {domain}"))?;

        Ok(rows
            .iter()
            .map(|r| {
                let url: String = r.get("url");
                let http_status: i32 = r.get("http_status");
                let content_length: i32 = r.get("content_length");
                let last_crawled_at: DateTime<Utc> = r.get("last_crawled_at");
                (url, http_status, content_length, last_crawled_at)
            })
            .collect())
    }

    /// Re-queue an entry with incremented attempt count for retry.
    /// Schedules the retry with exponential backoff based on attempt count.
    pub async fn requeue_with_retry(&self, entry: &CrawlQueueEntry) -> Result<()> {
        let now = Utc::now();

        // Exponential backoff: 1min, 2min, 4min, 8min, etc.
        let backoff_minutes = 2i64.pow(entry.attempt_count as u32);
        let scheduled_at = now + chrono::Duration::minutes(backoff_minutes);

        let new_entry = CrawlQueueEntry {
            queue_id: Uuid::now_v7(),
            tenant_id: entry.tenant_id,
            priority: entry.priority + 1, // Lower priority for retries
            scheduled_at,
            url: entry.url.clone(),
            domain: entry.domain.clone(),
            last_attempt_at: Some(now),
            attempt_count: entry.attempt_count + 1,
            created_at: entry.created_at,
        };

        self.insert_queue_entry(&new_entry).await
    }

    /// Get a snapshot of an allowed domain as JSON (for action history before_state).
    pub async fn get_allowed_domain_snapshot(
        &self,
        domain: &str,
    ) -> Result<Option<serde_json::Value>> {
        let row = sqlx::query(
            "SELECT domain, added_by, notes
             FROM allowed_domains
             WHERE tenant_id = $1 AND domain = $2 AND deleted_at IS NULL",
        )
        .bind(self.tenant_id)
        .bind(domain)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("Failed to snapshot domain: {domain}"))?;

        Ok(row.map(|r| {
            let domain: String = r.get("domain");
            let added_by: Option<String> = r.get("added_by");
            let notes: Option<String> = r.get("notes");
            serde_json::json!({
                "domain": domain,
                "added_by": added_by,
                "notes": notes,
            })
        }))
    }

    // ========== Action History Methods ==========

    /// Record a reversible action in the action history.
    #[allow(clippy::too_many_arguments)]
    pub async fn record_action(
        &self,
        entity_type: EntityType,
        action_type: ActionType,
        entity_id: &str,
        performed_by: Option<Uuid>,
        before_state: Option<&serde_json::Value>,
        after_state: Option<&serde_json::Value>,
        description: &str,
        rollback_of: Option<Uuid>,
    ) -> Result<ActionRecord> {
        let action_id = Uuid::now_v7();
        let entity_type_str = entity_type.to_string();
        let action_type_str = action_type.to_string();

        let row = sqlx::query(
            "INSERT INTO action_history
             (action_id, tenant_id, performed_by, entity_type, action_type, entity_id,
              before_state, after_state, description, rollback_of)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             RETURNING action_id, tenant_id, performed_by, performed_at, rolled_back_at,
                       rollback_of, entity_type, action_type, entity_id,
                       before_state, after_state, description",
        )
        .bind(action_id)
        .bind(self.tenant_id)
        .bind(performed_by)
        .bind(&entity_type_str)
        .bind(&action_type_str)
        .bind(entity_id)
        .bind(before_state)
        .bind(after_state)
        .bind(description)
        .bind(rollback_of)
        .fetch_one(&self.pool)
        .await
        .context("Failed to record action")?;

        Ok(row_to_action_record(&row))
    }

    /// Get the most recent non-rollback action that has not been rolled back (for Undo).
    pub async fn get_last_undoable_action(&self) -> Result<Option<ActionRecord>> {
        let row = sqlx::query(
            "SELECT action_id, tenant_id, performed_by, performed_at, rolled_back_at,
                    rollback_of, entity_type, action_type, entity_id,
                    before_state, after_state, description
             FROM action_history
             WHERE tenant_id = $1 AND rolled_back_at IS NULL AND action_type != 'rollback'
             ORDER BY performed_at DESC
             LIMIT 1",
        )
        .bind(self.tenant_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get last undoable action")?;

        Ok(row.as_ref().map(row_to_action_record))
    }

    /// Get the most recent rollback action that has not been rolled back (for Redo).
    pub async fn get_last_redoable_action(&self) -> Result<Option<ActionRecord>> {
        let row = sqlx::query(
            "SELECT action_id, tenant_id, performed_by, performed_at, rolled_back_at,
                    rollback_of, entity_type, action_type, entity_id,
                    before_state, after_state, description
             FROM action_history
             WHERE tenant_id = $1 AND rolled_back_at IS NULL AND action_type = 'rollback'
             ORDER BY performed_at DESC
             LIMIT 1",
        )
        .bind(self.tenant_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get last redoable action")?;

        Ok(row.as_ref().map(row_to_action_record))
    }

    /// List action history for the tenant, paginated, most recent first.
    pub async fn list_action_history(&self, limit: i64, offset: i64) -> Result<Vec<ActionRecord>> {
        let rows = sqlx::query(
            "SELECT action_id, tenant_id, performed_by, performed_at, rolled_back_at,
                    rollback_of, entity_type, action_type, entity_id,
                    before_state, after_state, description
             FROM action_history
             WHERE tenant_id = $1
             ORDER BY performed_at DESC
             LIMIT $2 OFFSET $3",
        )
        .bind(self.tenant_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .context("Failed to list action history")?;

        Ok(rows.iter().map(row_to_action_record).collect())
    }

    /// Mark an action as rolled back.
    pub async fn mark_action_rolled_back(&self, action_id: Uuid) -> Result<()> {
        sqlx::query(
            "UPDATE action_history SET rolled_back_at = now()
             WHERE action_id = $1 AND rolled_back_at IS NULL",
        )
        .bind(action_id)
        .execute(&self.pool)
        .await
        .with_context(|| format!("Failed to mark action rolled back: {action_id}"))?;
        Ok(())
    }

    /// Rollback an action by replaying from its before/after snapshots.
    /// Returns the new rollback ActionRecord.
    pub async fn rollback_action(
        &self,
        action: &ActionRecord,
        performed_by: Option<Uuid>,
    ) -> Result<ActionRecord> {
        if action.rolled_back_at.is_some() {
            bail!("Action {} has already been rolled back", action.action_id);
        }

        let original_action_type = ActionType::parse(&action.action_type);
        let entity_type = EntityType::parse(&action.entity_type);

        let (rollback_before, rollback_after) = match original_action_type {
            ActionType::Create => {
                self.apply_rollback_delete(&entity_type, &action.entity_id)
                    .await?;
                (action.after_state.clone(), None)
            }
            ActionType::Delete => {
                self.apply_rollback_restore(&entity_type, action).await?;
                (None, action.before_state.clone())
            }
            ActionType::Edit => {
                self.apply_rollback_edit(&entity_type, action).await?;
                (action.after_state.clone(), action.before_state.clone())
            }
            ActionType::Rollback => {
                self.apply_rollback_undo_rollback(&entity_type, action)
                    .await?;
                (action.after_state.clone(), action.before_state.clone())
            }
        };

        // Strip existing prefix so undo/redo cycles don't nest descriptions
        let base_desc = action
            .description
            .strip_prefix("Rolled back: ")
            .or_else(|| action.description.strip_prefix("Redone: "))
            .unwrap_or(&action.description);

        let description = if original_action_type == ActionType::Rollback {
            format!("Redone: {base_desc}")
        } else {
            format!("Rolled back: {base_desc}")
        };

        self.mark_action_rolled_back(action.action_id).await?;

        self.record_action(
            entity_type,
            ActionType::Rollback,
            &action.entity_id,
            performed_by,
            rollback_before.as_ref(),
            rollback_after.as_ref(),
            &description,
            Some(action.action_id),
        )
        .await
    }

    /// Apply rollback for a "create" action: soft-delete the entity.
    async fn apply_rollback_delete(&self, entity_type: &EntityType, entity_id: &str) -> Result<()> {
        match entity_type {
            EntityType::AllowedDomain => {
                self.delete_allowed_domain(entity_id).await?;
            }
            EntityType::Setting => {
                sqlx::query("DELETE FROM settings WHERE tenant_id = $1 AND setting_key = $2")
                    .bind(self.tenant_id)
                    .bind(entity_id)
                    .execute(&self.pool)
                    .await
                    .context("Failed to delete setting during rollback")?;
            }
            EntityType::OrgMembership => {
                let (tenant_id, user_id) = parse_membership_entity_id(entity_id)?;
                sqlx::query(
                    "UPDATE org_memberships SET deleted_at = now()
                     WHERE tenant_id = $1 AND user_id = $2 AND deleted_at IS NULL",
                )
                .bind(tenant_id)
                .bind(user_id)
                .execute(&self.pool)
                .await
                .context("Failed to soft-delete membership during rollback")?;
            }
        }
        Ok(())
    }

    /// Apply rollback for a "delete" action: restore from before_state snapshot.
    async fn apply_rollback_restore(
        &self,
        entity_type: &EntityType,
        action: &ActionRecord,
    ) -> Result<()> {
        let before = action
            .before_state
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No before_state to restore from"))?;

        match entity_type {
            EntityType::AllowedDomain => {
                let domain = before["domain"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing domain in before_state"))?;
                let added_by = before["added_by"].as_str().unwrap_or("rollback");
                let notes = before["notes"].as_str();
                self.insert_allowed_domain(domain, added_by, notes).await?;
            }
            EntityType::Setting => {
                let key = before["key"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing key in before_state"))?;
                let value = before["value"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing value in before_state"))?;
                self.set_setting(key, value).await?;
            }
            EntityType::OrgMembership => {
                let (tenant_id, user_id) = parse_membership_entity_id(&action.entity_id)?;
                let role = before["role"].as_str().unwrap_or("member");
                sqlx::query(
                    "UPDATE org_memberships SET deleted_at = NULL, role = $3
                     WHERE tenant_id = $1 AND user_id = $2",
                )
                .bind(tenant_id)
                .bind(user_id)
                .bind(role)
                .execute(&self.pool)
                .await
                .context("Failed to restore membership during rollback")?;
            }
        }
        Ok(())
    }

    /// Apply rollback for an "edit" action: restore before_state values.
    async fn apply_rollback_edit(
        &self,
        entity_type: &EntityType,
        action: &ActionRecord,
    ) -> Result<()> {
        self.apply_rollback_restore(entity_type, action).await
    }

    /// Apply rollback of a rollback: look at what the rollback did and reverse it.
    async fn apply_rollback_undo_rollback(
        &self,
        entity_type: &EntityType,
        action: &ActionRecord,
    ) -> Result<()> {
        // A rollback action's before_state = what was there before rollback ran
        // To undo the rollback, we restore before_state
        // But we need to figure out the direction:
        // If the rollback's after_state is Some (entity was restored), we need to delete it
        // If the rollback's after_state is None (entity was deleted), we need to restore from before_state
        if action.after_state.is_some() && action.before_state.is_none() {
            // The rollback restored something → undo by deleting
            self.apply_rollback_delete(entity_type, &action.entity_id)
                .await
        } else if action.before_state.is_some() && action.after_state.is_none() {
            // The rollback deleted something → undo by restoring
            self.apply_rollback_restore(entity_type, action).await
        } else {
            // Both present → it was an edit rollback, restore after_state (what was there before this rollback)
            // Swap: use after_state as the "before_state" to restore
            let swapped = ActionRecord {
                before_state: action.after_state.clone(),
                ..action.clone()
            };
            self.apply_rollback_restore(entity_type, &swapped).await
        }
    }

    /// Hard-delete an action history record (for test cleanup only).
    pub async fn hard_delete_action(&self, action_id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM action_history WHERE action_id = $1")
            .bind(action_id)
            .execute(&self.pool)
            .await
            .with_context(|| format!("Failed to hard-delete action: {action_id}"))?;
        Ok(())
    }
}

/// Parse a membership entity_id in the format "tenant_id:user_id".
fn parse_membership_entity_id(entity_id: &str) -> Result<(Uuid, Uuid)> {
    let parts: Vec<&str> = entity_id.split(':').collect();
    if parts.len() != 2 {
        bail!("Invalid membership entity_id format: {entity_id}");
    }
    let tenant_id = Uuid::parse_str(parts[0]).context("Invalid tenant_id in entity_id")?;
    let user_id = Uuid::parse_str(parts[1]).context("Invalid user_id in entity_id")?;
    Ok((tenant_id, user_id))
}

/// Convert a sqlx Row to an ActionRecord.
fn row_to_action_record(row: &sqlx::postgres::PgRow) -> ActionRecord {
    ActionRecord {
        action_id: row.get("action_id"),
        tenant_id: row.get("tenant_id"),
        performed_by: row.get("performed_by"),
        performed_at: row.get("performed_at"),
        rolled_back_at: row.get("rolled_back_at"),
        rollback_of: row.get("rollback_of"),
        entity_type: row.get("entity_type"),
        action_type: row.get("action_type"),
        entity_id: row.get("entity_id"),
        before_state: row.get("before_state"),
        after_state: row.get("after_state"),
        description: row.get("description"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Integration tests requiring PostgreSQL.
    // Run with: cargo test -- --ignored
    // Requires DATABASE_URL env variable.

    /// Helper to create a DbClient from environment variables.
    async fn create_test_client() -> DbClient {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for tests");
        let pool = PgPool::connect(&database_url)
            .await
            .expect("Failed to connect to PostgreSQL");

        // Use a test tenant
        let tenant_id = Uuid::now_v7();
        let client = DbClient::new(pool, tenant_id);

        client
            .ensure_default_tenant(tenant_id, "Test Tenant")
            .await
            .expect("Failed to create test tenant");

        client
    }

    #[tokio::test]
    #[ignore]
    async fn test_postgres_connection() {
        let _client = create_test_client().await;
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_next_queue_entry() {
        let client = create_test_client().await;
        let result = client.get_next_queue_entry().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[ignore]
    async fn test_is_domain_allowed_returns_false_for_unlisted_domain() {
        let client = create_test_client().await;

        let test_domain = format!(
            "test-unlisted-{}.example.invalid",
            chrono::Utc::now().timestamp_millis()
        );

        let result = client.is_domain_allowed(&test_domain).await;
        assert!(result.is_ok());
        assert!(!result.unwrap(), "Domain should not be allowed");
    }

    #[tokio::test]
    #[ignore]
    async fn test_is_domain_allowed_returns_true_for_listed_domain() {
        let client = create_test_client().await;

        let test_domain = format!(
            "test-allowed-{}.example.invalid",
            chrono::Utc::now().timestamp_millis()
        );

        client
            .insert_allowed_domain(&test_domain, "test", Some("Test domain"))
            .await
            .expect("Failed to insert test domain");

        let result = client.is_domain_allowed(&test_domain).await;
        assert!(result.is_ok());
        assert!(result.unwrap(), "Domain should be allowed");

        // Cleanup
        client
            .hard_delete_allowed_domain(&test_domain)
            .await
            .expect("Failed to clean up test domain");
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_and_set_setting() {
        let client = create_test_client().await;

        let test_key = format!("test_setting_{}", chrono::Utc::now().timestamp_millis());

        let result = client.get_setting(&test_key).await;
        assert!(result.is_ok());
        assert!(
            result.unwrap().is_none(),
            "Setting should not exist initially"
        );

        client
            .set_setting(&test_key, "test_value")
            .await
            .expect("Failed to set setting");

        let result = client.get_setting(&test_key).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some("test_value".to_string()));

        // Cleanup
        sqlx::query("DELETE FROM settings WHERE tenant_id = $1 AND setting_key = $2")
            .bind(client.tenant_id)
            .bind(&test_key)
            .execute(client.pool())
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_setting_returns_none_for_null_value() {
        let client = create_test_client().await;
        let test_key = format!(
            "test_setting_null_{}",
            chrono::Utc::now().timestamp_millis()
        );

        sqlx::query(
            "INSERT INTO settings (tenant_id, setting_key, setting_value, updated_at)
             VALUES ($1, $2, NULL, now())
             ON CONFLICT (tenant_id, setting_key) DO UPDATE SET setting_value = NULL, updated_at = now()",
        )
        .bind(client.tenant_id)
        .bind(&test_key)
        .execute(client.pool())
        .await
        .expect("insert null setting");

        let result = client.get_setting(&test_key).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);

        sqlx::query("DELETE FROM settings WHERE tenant_id = $1 AND setting_key = $2")
            .bind(client.tenant_id)
            .bind(&test_key)
            .execute(client.pool())
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_crawling_enabled_flag() {
        let client = create_test_client().await;

        client
            .set_crawling_enabled(true)
            .await
            .expect("Failed to enable crawling");

        let result = client.is_crawling_enabled().await;
        assert!(result.is_ok());
        assert!(result.unwrap(), "Crawling should be enabled");

        client
            .set_crawling_enabled(false)
            .await
            .expect("Failed to disable crawling");

        let result = client.is_crawling_enabled().await;
        assert!(result.is_ok());
        assert!(!result.unwrap(), "Crawling should be disabled");
    }

    #[tokio::test]
    #[ignore]
    async fn test_index_capacity_usage_sums_indexed_text_bytes() {
        let client = create_test_client().await;

        unsafe { std::env::set_var("TENANT_INDEX_CAPACITY_BYTES", "10000") };

        let test_domain = format!(
            "test-usage-{}.example.invalid",
            chrono::Utc::now().timestamp_millis()
        );
        let now = Utc::now();

        let first_page = CrawledPage {
            page_id: Uuid::now_v7(),
            tenant_id: client.tenant_id,
            domain: test_domain.clone(),
            url_path: "/first".to_string(),
            url: format!("https://{}/first", test_domain),
            storage_id: None,
            storage_compression: CompressionType::None,
            last_crawled_at: now,
            next_crawl_at: now + chrono::Duration::hours(24),
            crawl_frequency_hours: 24,
            http_status: 200,
            content_hash: "first".to_string(),
            content_length: 9000,
            indexed_document_bytes: Some(1200),
            robots_allowed: true,
            error_message: None,
            crawl_count: 1,
            created_at: now,
            updated_at: now,
        };
        let second_page = CrawledPage {
            page_id: Uuid::now_v7(),
            tenant_id: client.tenant_id,
            domain: test_domain.clone(),
            url_path: "/second".to_string(),
            url: format!("https://{}/second", test_domain),
            content_hash: "second".to_string(),
            content_length: 14000,
            indexed_document_bytes: Some(3400),
            ..first_page.clone()
        };

        client
            .upsert_crawled_page(&first_page)
            .await
            .expect("insert first page");
        client
            .upsert_crawled_page(&second_page)
            .await
            .expect("insert second page");

        let usage = client
            .get_index_usage_bytes()
            .await
            .expect("get index usage");
        assert_eq!(usage, 4600);

        let max_bytes = client
            .get_index_capacity_bytes()
            .await
            .expect("get index capacity");
        assert_eq!(max_bytes, 10000);

        client
            .set_index_capacity_bytes(2048)
            .await
            .expect("set index capacity override");

        let overridden = client
            .get_index_capacity_bytes()
            .await
            .expect("get overridden capacity");
        assert_eq!(overridden, 2048);

        client
            .delete_crawled_page(&test_domain, "/first")
            .await
            .expect("cleanup first page");
        client
            .delete_crawled_page(&test_domain, "/second")
            .await
            .expect("cleanup second page");
        sqlx::query("DELETE FROM settings WHERE tenant_id = $1 AND setting_key = $2")
            .bind(client.tenant_id)
            .bind(INDEX_CAPACITY_SETTING_KEY)
            .execute(client.pool())
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_recent_crawled_pages_returns_pages_sorted_by_crawled_at() {
        let client = create_test_client().await;

        let test_domain = format!(
            "test-recent-{}.example.invalid",
            chrono::Utc::now().timestamp_millis()
        );

        let now = Utc::now();
        let older_page = CrawledPage {
            page_id: Uuid::now_v7(),
            tenant_id: client.tenant_id,
            domain: test_domain.clone(),
            url_path: "/old".to_string(),
            url: format!("https://{}/old", test_domain),
            storage_id: None,
            storage_compression: CompressionType::None,
            last_crawled_at: now - chrono::Duration::hours(1),
            next_crawl_at: now + chrono::Duration::hours(24),
            crawl_frequency_hours: 24,
            http_status: 200,
            content_hash: "abc123".to_string(),
            content_length: 5000,
            indexed_document_bytes: Some(4200),
            robots_allowed: true,
            error_message: None,
            crawl_count: 1,
            created_at: now - chrono::Duration::hours(1),
            updated_at: now - chrono::Duration::hours(1),
        };
        let newer_page = CrawledPage {
            page_id: Uuid::now_v7(),
            tenant_id: client.tenant_id,
            domain: test_domain.clone(),
            url_path: "/new".to_string(),
            url: format!("https://{}/new", test_domain),
            last_crawled_at: now,
            content_hash: "def456".to_string(),
            content_length: 8000,
            created_at: now,
            updated_at: now,
            ..older_page.clone()
        };

        client
            .upsert_crawled_page(&older_page)
            .await
            .expect("Failed to insert older page");
        client
            .upsert_crawled_page(&newer_page)
            .await
            .expect("Failed to insert newer page");

        let result = client
            .get_recent_crawled_pages(&test_domain, 10)
            .await
            .expect("Failed to get recent pages");

        assert_eq!(result.len(), 2);
        // First result should be newer (DESC order)
        assert!(result[0].0.contains("/new"));
        assert!(result[1].0.contains("/old"));
        assert_eq!(result[0].1, 200); // http_status
        assert_eq!(result[0].2, 8000); // content_length

        // Cleanup
        client
            .delete_crawled_page(&test_domain, "/old")
            .await
            .expect("Failed to clean up");
        client
            .delete_crawled_page(&test_domain, "/new")
            .await
            .expect("Failed to clean up");
    }

    #[tokio::test]
    #[ignore]
    async fn test_ensure_default_tenant_is_idempotent() {
        let client = create_test_client().await;

        // Should be idempotent - calling twice should not error
        client
            .ensure_default_tenant(client.tenant_id, "Test")
            .await
            .expect("First call should succeed");
        client
            .ensure_default_tenant(client.tenant_id, "Test")
            .await
            .expect("Second call should also succeed (ON CONFLICT DO NOTHING)");

        // Verify the tenant row exists
        let row = sqlx::query("SELECT tenant_id FROM tenants WHERE tenant_id = $1")
            .bind(client.tenant_id)
            .fetch_optional(client.pool())
            .await
            .unwrap();
        assert!(
            row.is_some(),
            "Tenant row should exist after ensure_default_tenant"
        );
    }

    // ========== Action History Tests ==========

    use crate::models::action_history::{ActionType, EntityType};

    #[tokio::test]
    #[ignore]
    async fn test_record_action_inserts_and_returns_correct_fields() {
        let client = create_test_client().await;

        let after_state = serde_json::json!({"domain": "example.com", "added_by": "test"});

        let record = client
            .record_action(
                EntityType::AllowedDomain,
                ActionType::Create,
                "example.com",
                None,
                None,
                Some(&after_state),
                "Added domain example.com",
                None,
            )
            .await
            .expect("Failed to record action");

        assert_eq!(record.entity_type, "allowed_domain");
        assert_eq!(record.action_type, "create");
        assert_eq!(record.entity_id, "example.com");
        assert_eq!(record.description, "Added domain example.com");
        assert!(record.rolled_back_at.is_none());
        assert!(record.rollback_of.is_none());
        assert!(record.before_state.is_none());
        assert_eq!(record.after_state, Some(after_state));

        // Cleanup
        client.hard_delete_action(record.action_id).await.ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_last_undoable_returns_most_recent_active() {
        let client = create_test_client().await;

        let r1 = client
            .record_action(
                EntityType::AllowedDomain,
                ActionType::Create,
                "first.com",
                None,
                None,
                None,
                "Added first.com",
                None,
            )
            .await
            .expect("record r1");

        let r2 = client
            .record_action(
                EntityType::AllowedDomain,
                ActionType::Create,
                "second.com",
                None,
                None,
                None,
                "Added second.com",
                None,
            )
            .await
            .expect("record r2");

        let last = client
            .get_last_undoable_action()
            .await
            .expect("get last undoable")
            .expect("should have an undoable action");

        assert_eq!(last.action_id, r2.action_id);

        // Cleanup
        client.hard_delete_action(r2.action_id).await.ok();
        client.hard_delete_action(r1.action_id).await.ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_last_undoable_skips_rolled_back() {
        let client = create_test_client().await;

        let r1 = client
            .record_action(
                EntityType::AllowedDomain,
                ActionType::Create,
                "first.com",
                None,
                None,
                None,
                "Added first.com",
                None,
            )
            .await
            .expect("record r1");

        let r2 = client
            .record_action(
                EntityType::AllowedDomain,
                ActionType::Create,
                "second.com",
                None,
                None,
                None,
                "Added second.com",
                None,
            )
            .await
            .expect("record r2");

        // Mark r2 as rolled back
        client
            .mark_action_rolled_back(r2.action_id)
            .await
            .expect("mark rolled back");

        let last = client
            .get_last_undoable_action()
            .await
            .expect("get last undoable")
            .expect("should have an undoable action");

        // Should skip r2 (rolled back) and return r1
        assert_eq!(last.action_id, r1.action_id);

        // Cleanup
        client.hard_delete_action(r2.action_id).await.ok();
        client.hard_delete_action(r1.action_id).await.ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_rollback_create_soft_deletes_domain() {
        let client = create_test_client().await;

        let test_domain = format!(
            "test-rollback-create-{}.example.invalid",
            chrono::Utc::now().timestamp_millis()
        );

        client
            .insert_allowed_domain(&test_domain, "test", None)
            .await
            .expect("insert domain");

        let after_state =
            serde_json::json!({"domain": test_domain, "added_by": "test", "notes": null});

        let action = client
            .record_action(
                EntityType::AllowedDomain,
                ActionType::Create,
                &test_domain,
                None,
                None,
                Some(&after_state),
                &format!("Added domain {test_domain}"),
                None,
            )
            .await
            .expect("record action");

        // Rollback the create → should soft-delete the domain
        let rollback = client
            .rollback_action(&action, None)
            .await
            .expect("rollback action");

        assert_eq!(rollback.action_type, "rollback");
        assert_eq!(rollback.rollback_of, Some(action.action_id));

        // Domain should no longer be active
        let is_allowed = client
            .is_domain_allowed(&test_domain)
            .await
            .expect("check domain");
        assert!(!is_allowed, "Domain should be soft-deleted after rollback");

        // Cleanup
        client.hard_delete_allowed_domain(&test_domain).await.ok();
        client.hard_delete_action(rollback.action_id).await.ok();
        client.hard_delete_action(action.action_id).await.ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_rollback_delete_restores_domain() {
        let client = create_test_client().await;

        let test_domain = format!(
            "test-rollback-delete-{}.example.invalid",
            chrono::Utc::now().timestamp_millis()
        );

        client
            .insert_allowed_domain(&test_domain, "test", Some("test notes"))
            .await
            .expect("insert domain");

        let before_state = serde_json::json!({
            "domain": test_domain,
            "added_by": "test",
            "notes": "test notes"
        });

        client
            .delete_allowed_domain(&test_domain)
            .await
            .expect("delete domain");

        let action = client
            .record_action(
                EntityType::AllowedDomain,
                ActionType::Delete,
                &test_domain,
                None,
                Some(&before_state),
                None,
                &format!("Removed domain {test_domain}"),
                None,
            )
            .await
            .expect("record action");

        // Rollback the delete → should restore the domain
        let rollback = client
            .rollback_action(&action, None)
            .await
            .expect("rollback action");

        assert_eq!(rollback.action_type, "rollback");

        // Domain should be active again
        let is_allowed = client
            .is_domain_allowed(&test_domain)
            .await
            .expect("check domain");
        assert!(is_allowed, "Domain should be restored after rollback");

        // Cleanup
        client.hard_delete_allowed_domain(&test_domain).await.ok();
        client.hard_delete_action(rollback.action_id).await.ok();
        client.hard_delete_action(action.action_id).await.ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_rollback_of_rollback_reapplies_original() {
        let client = create_test_client().await;

        let test_domain = format!(
            "test-rollback-rollback-{}.example.invalid",
            chrono::Utc::now().timestamp_millis()
        );

        client
            .insert_allowed_domain(&test_domain, "test", None)
            .await
            .expect("insert domain");

        let after_state =
            serde_json::json!({"domain": test_domain, "added_by": "test", "notes": null});

        let create_action = client
            .record_action(
                EntityType::AllowedDomain,
                ActionType::Create,
                &test_domain,
                None,
                None,
                Some(&after_state),
                &format!("Added domain {test_domain}"),
                None,
            )
            .await
            .expect("record create");

        // Rollback the create (soft-deletes domain)
        let rollback1 = client
            .rollback_action(&create_action, None)
            .await
            .expect("rollback create");

        assert!(
            !client.is_domain_allowed(&test_domain).await.expect("check"),
            "Domain should be gone after first rollback"
        );

        // Rollback the rollback (restores domain)
        let rollback2 = client
            .rollback_action(&rollback1, None)
            .await
            .expect("rollback rollback");

        assert_eq!(rollback2.rollback_of, Some(rollback1.action_id));
        assert!(
            client.is_domain_allowed(&test_domain).await.expect("check"),
            "Domain should be restored after rollback-of-rollback"
        );

        // Cleanup
        client.hard_delete_allowed_domain(&test_domain).await.ok();
        client.hard_delete_action(rollback2.action_id).await.ok();
        client.hard_delete_action(rollback1.action_id).await.ok();
        client
            .hard_delete_action(create_action.action_id)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_double_rollback_returns_error() {
        let client = create_test_client().await;

        let action = client
            .record_action(
                EntityType::AllowedDomain,
                ActionType::Create,
                "double.com",
                None,
                None,
                None,
                "Added double.com",
                None,
            )
            .await
            .expect("record action");

        // First rollback should succeed
        let rollback = client
            .rollback_action(&action, None)
            .await
            .expect("first rollback");

        // Create another action and mark it rolled back to test double-rollback
        let stale_action = client
            .record_action(
                EntityType::AllowedDomain,
                ActionType::Create,
                "double2.com",
                None,
                None,
                None,
                "dummy",
                None,
            )
            .await
            .expect("dummy");

        client
            .mark_action_rolled_back(stale_action.action_id)
            .await
            .expect("mark");

        let mut already_rolled_back = stale_action;
        already_rolled_back.rolled_back_at = Some(Utc::now());

        let result = client.rollback_action(&already_rolled_back, None).await;
        assert!(
            result.is_err(),
            "Rolling back an already-rolled-back action should fail"
        );

        // Cleanup
        client
            .hard_delete_action(already_rolled_back.action_id)
            .await
            .ok();
        client.hard_delete_action(rollback.action_id).await.ok();
        client.hard_delete_action(action.action_id).await.ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_list_action_history_returns_paginated_desc() {
        let client = create_test_client().await;

        // Insert 3 actions
        let mut ids = Vec::new();
        for i in 0..3 {
            let r = client
                .record_action(
                    EntityType::AllowedDomain,
                    ActionType::Create,
                    &format!("domain{i}.com"),
                    None,
                    None,
                    None,
                    &format!("Added domain{i}.com"),
                    None,
                )
                .await
                .expect("record");
            ids.push(r.action_id);
        }

        // List with limit=2, offset=0 → should get the 2 most recent
        let page1 = client.list_action_history(2, 0).await.expect("list page 1");
        assert_eq!(page1.len(), 2);
        assert_eq!(page1[0].action_id, ids[2]); // most recent first
        assert_eq!(page1[1].action_id, ids[1]);

        // List with limit=2, offset=2 → should get the oldest
        let page2 = client.list_action_history(2, 2).await.expect("list page 2");
        assert_eq!(page2.len(), 1);
        assert_eq!(page2[0].action_id, ids[0]);

        // Cleanup
        for id in ids {
            client.hard_delete_action(id).await.ok();
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_rollback_setting_change_restores_previous_value() {
        let client = create_test_client().await;

        let test_key = format!("test_setting_{}", chrono::Utc::now().timestamp_millis());

        // Set initial value
        client
            .set_setting(&test_key, "true")
            .await
            .expect("set initial");

        let before_state = serde_json::json!({"key": test_key, "value": "true"});
        let after_state = serde_json::json!({"key": test_key, "value": "false"});

        // Change setting
        client
            .set_setting(&test_key, "false")
            .await
            .expect("change setting");

        let action = client
            .record_action(
                EntityType::Setting,
                ActionType::Edit,
                &test_key,
                None,
                Some(&before_state),
                Some(&after_state),
                &format!("Changed {test_key} from true to false"),
                None,
            )
            .await
            .expect("record action");

        // Rollback → should restore "true"
        let rollback = client
            .rollback_action(&action, None)
            .await
            .expect("rollback");

        let value = client
            .get_setting(&test_key)
            .await
            .expect("get setting")
            .expect("setting should exist");
        assert_eq!(value, "true", "Setting should be restored to before_state");

        // Cleanup
        sqlx::query("DELETE FROM settings WHERE tenant_id = $1 AND setting_key = $2")
            .bind(client.tenant_id)
            .bind(&test_key)
            .execute(client.pool())
            .await
            .ok();
        client.hard_delete_action(rollback.action_id).await.ok();
        client.hard_delete_action(action.action_id).await.ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_last_undoable_skips_rollback_actions() {
        let client = create_test_client().await;

        let test_domain = format!(
            "test-undo-skip-{}.example.invalid",
            chrono::Utc::now().timestamp_millis()
        );

        // Create domain and record the action
        client
            .insert_allowed_domain(&test_domain, "test", None)
            .await
            .expect("insert");

        let after_state =
            serde_json::json!({"domain": test_domain, "added_by": "test", "notes": null});

        let create_action = client
            .record_action(
                EntityType::AllowedDomain,
                ActionType::Create,
                &test_domain,
                None,
                None,
                Some(&after_state),
                &format!("Added domain {test_domain}"),
                None,
            )
            .await
            .expect("record create");

        // Undo the create (this creates a rollback action)
        let rollback = client
            .rollback_action(&create_action, None)
            .await
            .expect("rollback");

        // last_undoable should NOT return the rollback action
        // (rollback actions are for redo, not undo)
        let last_undoable = client.get_last_undoable_action().await.expect("get");
        assert!(
            last_undoable.is_none(),
            "Should not return rollback action as undoable"
        );

        // Cleanup
        client.hard_delete_allowed_domain(&test_domain).await.ok();
        client.hard_delete_action(rollback.action_id).await.ok();
        client
            .hard_delete_action(create_action.action_id)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_last_redoable_returns_most_recent_rollback() {
        let client = create_test_client().await;

        let test_domain = format!(
            "test-redo-{}.example.invalid",
            chrono::Utc::now().timestamp_millis()
        );

        // Create domain and record the action
        client
            .insert_allowed_domain(&test_domain, "test", None)
            .await
            .expect("insert");

        let after_state =
            serde_json::json!({"domain": test_domain, "added_by": "test", "notes": null});

        let create_action = client
            .record_action(
                EntityType::AllowedDomain,
                ActionType::Create,
                &test_domain,
                None,
                None,
                Some(&after_state),
                &format!("Added domain {test_domain}"),
                None,
            )
            .await
            .expect("record create");

        // Undo the create
        let rollback = client
            .rollback_action(&create_action, None)
            .await
            .expect("rollback");

        // last_redoable should return the rollback action
        let last_redoable = client.get_last_redoable_action().await.expect("get");
        assert!(last_redoable.is_some(), "Should have a redoable action");
        assert_eq!(last_redoable.unwrap().action_id, rollback.action_id);

        // Cleanup
        client.hard_delete_allowed_domain(&test_domain).await.ok();
        client.hard_delete_action(rollback.action_id).await.ok();
        client
            .hard_delete_action(create_action.action_id)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_undo_redo_cycle_clean_descriptions() {
        let client = create_test_client().await;

        let test_domain = format!(
            "test-undo-redo-{}.example.invalid",
            chrono::Utc::now().timestamp_millis()
        );

        // Create domain
        client
            .insert_allowed_domain(&test_domain, "test", None)
            .await
            .expect("insert");

        let after_state =
            serde_json::json!({"domain": test_domain, "added_by": "test", "notes": null});

        let create_action = client
            .record_action(
                EntityType::AllowedDomain,
                ActionType::Create,
                &test_domain,
                None,
                None,
                Some(&after_state),
                &format!("Added domain {test_domain}"),
                None,
            )
            .await
            .expect("record create");

        // Undo
        let undo = client
            .rollback_action(&create_action, None)
            .await
            .expect("undo");

        // Description should reference original, not nest
        assert!(
            !undo.description.contains("Rolled back: Rolled back:"),
            "Description should not nest: {}",
            undo.description
        );

        // Redo (rollback the undo)
        let redo = client.rollback_action(&undo, None).await.expect("redo");

        // Description should still be clean
        assert!(
            !redo.description.contains("Rolled back: Rolled back:"),
            "Redo description should not nest: {}",
            redo.description
        );

        // Undo again
        let undo2 = client.rollback_action(&redo, None).await.expect("undo2");

        // Still no nesting
        assert!(
            !undo2.description.contains("Rolled back: Rolled back:"),
            "Second undo description should not nest: {}",
            undo2.description
        );

        // Cleanup
        client.hard_delete_allowed_domain(&test_domain).await.ok();
        client.hard_delete_action(undo2.action_id).await.ok();
        client.hard_delete_action(redo.action_id).await.ok();
        client.hard_delete_action(undo.action_id).await.ok();
        client
            .hard_delete_action(create_action.action_id)
            .await
            .ok();
    }
}
