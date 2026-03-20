// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

//! Application state, per-request tenant DB resolution, route handlers, and router
//! construction.
//!
//! This module is `pub` so that integration tests can build a test router directly
//! without starting the full binary.

use crate::models::action_history::{
    ActionType, EntityType, RollbackResponse, UndoRedoStateResponse,
};
use crate::models::db::CrawlQueueEntry;
use crate::models::deployment::DeploymentMode;
use crate::models::domain::{
    AddDomainRequest, AddDomainResponse, DeleteDomainResponse, DomainInfo, ListDomainsResponse,
};
use crate::models::onboarding::{RecentPageInfo, RecentPagesQuery, RecentPagesResponse};
use crate::models::queue::{AddToQueueRequest, AddToQueueResponse};
use crate::models::search::{SearchRequest, SearchResponse};
use crate::models::settings::{CrawlingEnabledResponse, SetCrawlingEnabledRequest};
use crate::models::version::VersionResponse;
use crate::routes::{auth_router, AuthApiDoc, AuthState};
use crate::services::db::DbClient;
use crate::services::search::SearchClient;
use axum::{
    extract::{FromRequestParts, Path, Query, State},
    http::{request::Parts, StatusCode},
    routing::{delete, get, post, put},
    Json, Router,
};
use chrono::Utc;
use std::sync::Arc;
use tower_cookies::{CookieManagerLayer, Cookies};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;
use uuid::Uuid;

/// Application version extracted from `Cargo.toml` at compile time.
/// The patch segment can be overridden via `LALA_PATCH_VERSION` (see `build.rs`).
pub const VERSION: &str = env!("LALA_VERSION");

// ---------------------------------------------------------------------------
// Application state
// ---------------------------------------------------------------------------

/// Shared application state injected into every route handler via `State<AppState>`.
#[derive(Clone)]
pub struct AppState {
    /// Base database client. Used as-is in single-tenant mode; used as the
    /// connection-pool source for `with_tenant()` in multi-tenant mode.
    pub db_client: Arc<DbClient>,
    pub search_client: Option<Arc<SearchClient>>,
    pub deployment_mode: DeploymentMode,
    /// Required in multi-tenant mode: validates session cookies and resolves
    /// the authenticated user's tenant.
    pub auth_state: Option<AuthState>,
}

// ---------------------------------------------------------------------------
// Per-request tenant DB extractor
// ---------------------------------------------------------------------------

/// Axum extractor that resolves the database client to use for the current request.
///
/// When `auth_state` is configured, the session cookie is validated in **both**
/// single-tenant and multi-tenant modes.  The only difference is how the DB
/// client is selected after authentication succeeds:
///
/// * **Single-tenant**: returns `state.db_client` directly.
/// * **Multi-tenant**: returns `state.db_client.with_tenant(tenant_id)` where
///   `tenant_id` comes from the authenticated session.
///
/// When `auth_state` is `None` (email not configured), routes are open and the
/// default `db_client` is returned without authentication.
pub struct TenantDb(pub Arc<DbClient>);

impl FromRequestParts<AppState> for TenantDb {
    type Rejection = (StatusCode, String);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // No auth configured → open access with default db
        if state.auth_state.is_none() {
            return Ok(TenantDb(state.db_client.clone()));
        }

        if state.deployment_mode == DeploymentMode::SingleTenant {
            // Validate session but return the default db_client
            validate_session(parts, state).await?;
            return Ok(TenantDb(state.db_client.clone()));
        }

        // Multi-tenant: validate session and resolve tenant
        resolve_multi_tenant_db(parts, state).await.map(TenantDb)
    }
}

/// Validate the session cookie and return the authenticated user's tenant ID.
///
/// Requires `auth_state` to be configured; returns 401 if the session cookie
/// is missing or invalid.
async fn validate_session(
    parts: &mut Parts,
    state: &AppState,
) -> Result<Uuid, (StatusCode, String)> {
    let auth_state = state.auth_state.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "Auth service not configured".to_string(),
        )
    })?;

    let cookies = Cookies::from_request_parts(parts, state)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to read request cookies".to_string(),
            )
        })?;

    let session_token = cookies
        .get("lala_session")
        .map(|c| c.value().to_string())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                "Authentication required".to_string(),
            )
        })?;

    let auth_user = auth_state
        .auth_service
        .validate_session(&session_token)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Session validation error: {e}"),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                "Invalid or expired session".to_string(),
            )
        })?;

    Ok(auth_user.tenant_id)
}

/// Resolve the tenant-scoped DB client for a multi-tenant request.
async fn resolve_multi_tenant_db(
    parts: &mut Parts,
    state: &AppState,
) -> Result<Arc<DbClient>, (StatusCode, String)> {
    let tenant_id = validate_session(parts, state).await?;
    Ok(Arc::new(state.db_client.with_tenant(tenant_id)))
}

// ---------------------------------------------------------------------------
// Route handlers
// ---------------------------------------------------------------------------

pub async fn version_handler(State(state): State<AppState>) -> Json<VersionResponse> {
    Json(VersionResponse {
        agent: "lala-agent".to_string(),
        version: VERSION.to_string(),
        deployment_mode: state.deployment_mode.to_string(),
    })
}

pub async fn add_to_queue_handler(
    TenantDb(db): TenantDb,
    Json(payload): Json<AddToQueueRequest>,
) -> Result<Json<AddToQueueResponse>, (StatusCode, String)> {
    let parsed_url = url::Url::parse(&payload.url)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid URL: {e}")))?;

    let domain = parsed_url
        .host_str()
        .ok_or((StatusCode::BAD_REQUEST, "URL has no host".to_string()))?
        .to_string();

    let is_allowed = db.is_domain_allowed(&domain).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to check domain allowlist: {e}"),
        )
    })?;

    if !is_allowed {
        return Err((
            StatusCode::FORBIDDEN,
            format!("Domain '{domain}' is not in the allowed domains list"),
        ));
    }

    let now = Utc::now();
    let entry = CrawlQueueEntry {
        queue_id: Uuid::now_v7(),
        tenant_id: db.tenant_id,
        priority: payload.priority,
        scheduled_at: now,
        url: payload.url.clone(),
        domain: domain.clone(),
        last_attempt_at: None,
        attempt_count: 0,
        created_at: now,
    };

    db.insert_queue_entry(&entry).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {e}"),
        )
    })?;

    Ok(Json(AddToQueueResponse {
        success: true,
        message: "URL added to crawl queue successfully".to_string(),
        url: payload.url,
        domain,
    }))
}

/// Extractor that resolves the tenant_id for search filtering.
/// Returns `Some(tenant_id)` in multi-tenant mode with auth, `None` otherwise.
pub struct SearchTenantId(pub Option<String>);

impl FromRequestParts<AppState> for SearchTenantId {
    type Rejection = (StatusCode, String);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if state.deployment_mode == DeploymentMode::MultiTenant && state.auth_state.is_some() {
            let tenant_id = validate_session(parts, state).await?;
            Ok(SearchTenantId(Some(tenant_id.to_string())))
        } else {
            Ok(SearchTenantId(None))
        }
    }
}

pub async fn search_handler(
    SearchTenantId(tenant_id): SearchTenantId,
    State(state): State<AppState>,
    Json(payload): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, (StatusCode, String)> {
    let search_client = state.search_client.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "Search service is not available".to_string(),
        )
    })?;

    search_client
        .search(payload, tenant_id.as_deref())
        .await
        .map(Json)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Search error: {e}"),
            )
        })
}

pub async fn add_domain_handler(
    TenantDb(db): TenantDb,
    Json(payload): Json<AddDomainRequest>,
) -> Result<Json<AddDomainResponse>, (StatusCode, String)> {
    if payload.domain.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Domain cannot be empty".to_string(),
        ));
    }

    db.insert_allowed_domain(&payload.domain, "api", payload.notes.as_deref())
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {e}"),
            )
        })?;

    let after_state = serde_json::json!({
        "domain": payload.domain,
        "added_by": "api",
        "notes": payload.notes,
    });

    let action_id = db
        .record_action(
            EntityType::AllowedDomain,
            ActionType::Create,
            &payload.domain,
            None,
            None,
            Some(&after_state),
            &format!("Added domain {}", payload.domain),
            None,
        )
        .await
        .ok()
        .map(|r| r.action_id.to_string());

    Ok(Json(AddDomainResponse {
        success: true,
        message: "Domain added to allowed list successfully".to_string(),
        domain: payload.domain,
        action_id,
    }))
}

pub async fn list_domains_handler(
    TenantDb(db): TenantDb,
) -> Result<Json<ListDomainsResponse>, (StatusCode, String)> {
    let domains = db.list_allowed_domains().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {e}"),
        )
    })?;

    let domain_infos: Vec<DomainInfo> = domains
        .into_iter()
        .map(|(domain, added_by, notes, added_at)| DomainInfo {
            domain,
            added_at,
            added_by,
            notes,
        })
        .collect();

    let count = domain_infos.len();
    Ok(Json(ListDomainsResponse {
        domains: domain_infos,
        count,
    }))
}

pub async fn delete_domain_handler(
    TenantDb(db): TenantDb,
    Path(domain): Path<String>,
) -> Result<Json<DeleteDomainResponse>, (StatusCode, String)> {
    // Snapshot before delete for rollback
    let before_state = db.get_allowed_domain_snapshot(&domain).await.ok().flatten();

    db.delete_allowed_domain(&domain).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {e}"),
        )
    })?;

    let action_id = db
        .record_action(
            EntityType::AllowedDomain,
            ActionType::Delete,
            &domain,
            None,
            before_state.as_ref(),
            None,
            &format!("Removed domain {domain}"),
            None,
        )
        .await
        .ok()
        .map(|r| r.action_id.to_string());

    Ok(Json(DeleteDomainResponse {
        success: true,
        message: "Domain removed from allowed list successfully".to_string(),
        domain,
        action_id,
    }))
}

pub async fn get_crawling_enabled_handler(
    TenantDb(db): TenantDb,
) -> Result<Json<CrawlingEnabledResponse>, (StatusCode, String)> {
    let enabled = db.is_crawling_enabled().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {e}"),
        )
    })?;

    Ok(Json(CrawlingEnabledResponse {
        enabled,
        action_id: None,
    }))
}

pub async fn set_crawling_enabled_handler(
    TenantDb(db): TenantDb,
    Json(payload): Json<SetCrawlingEnabledRequest>,
) -> Result<Json<CrawlingEnabledResponse>, (StatusCode, String)> {
    // Snapshot before change
    let old_enabled = db.is_crawling_enabled().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {e}"),
        )
    })?;

    db.set_crawling_enabled(payload.enabled)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {e}"),
            )
        })?;

    let before_state =
        serde_json::json!({"key": "crawling_enabled", "value": old_enabled.to_string()});
    let after_state =
        serde_json::json!({"key": "crawling_enabled", "value": payload.enabled.to_string()});

    let action_id = db
        .record_action(
            EntityType::Setting,
            ActionType::Edit,
            "crawling_enabled",
            None,
            Some(&before_state),
            Some(&after_state),
            &format!(
                "Changed crawling from {} to {}",
                if old_enabled { "enabled" } else { "disabled" },
                if payload.enabled {
                    "enabled"
                } else {
                    "disabled"
                }
            ),
            None,
        )
        .await
        .ok()
        .map(|r| r.action_id.to_string());

    Ok(Json(CrawlingEnabledResponse {
        enabled: payload.enabled,
        action_id,
    }))
}

pub async fn recent_crawled_pages_handler(
    TenantDb(db): TenantDb,
    State(state): State<AppState>,
    Query(params): Query<RecentPagesQuery>,
) -> Result<Json<RecentPagesResponse>, (StatusCode, String)> {
    if params.domain.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "domain parameter is required".to_string(),
        ));
    }

    let limit = params.limit.unwrap_or(10).min(50) as i64;

    // Query the database for recently crawled pages
    let db_pages = db
        .get_recent_crawled_pages(&params.domain, limit)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {e}"),
            )
        })?;

    let total = db_pages.len() as u32;

    let mut pages: Vec<RecentPageInfo> = db_pages
        .into_iter()
        .map(
            |(url, http_status, content_length, last_crawled_at)| RecentPageInfo {
                url,
                http_status,
                content_length,
                last_crawled_at: last_crawled_at.timestamp(),
                title: None,
                excerpt: None,
            },
        )
        .collect();

    // Optionally enrich with titles/excerpts from Meilisearch
    if params.enrich.unwrap_or(false) {
        if let Some(search_client) = state.search_client.as_ref() {
            let tenant_id_str = db.tenant_id.to_string();
            let tenant_filter = if state.deployment_mode == DeploymentMode::MultiTenant {
                Some(tenant_id_str.as_str())
            } else {
                None
            };

            if let Ok(docs) = search_client
                .list_by_domain(&params.domain, tenant_filter, limit as usize)
                .await
            {
                // Build URL → (title, excerpt) map for O(1) lookup
                let enrichment: std::collections::HashMap<String, (Option<String>, String)> = docs
                    .into_iter()
                    .map(|doc| (doc.url.clone(), (doc.title, doc.excerpt)))
                    .collect();

                for page in &mut pages {
                    if let Some((title, excerpt)) = enrichment.get(&page.url) {
                        page.title.clone_from(title);
                        page.excerpt = Some(excerpt.clone());
                    }
                }
            }
        }
    }

    Ok(Json(RecentPagesResponse { pages, total }))
}

// ---------------------------------------------------------------------------
// Action History Handlers
// ---------------------------------------------------------------------------

pub async fn undo_redo_state_handler(
    TenantDb(db): TenantDb,
) -> Result<Json<UndoRedoStateResponse>, (StatusCode, String)> {
    let undoable = db.get_last_undoable_action().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {e}"),
        )
    })?;

    let redoable = db.get_last_redoable_action().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {e}"),
        )
    })?;

    Ok(Json(UndoRedoStateResponse { undoable, redoable }))
}

pub async fn undo_last_handler(
    TenantDb(db): TenantDb,
) -> Result<Json<RollbackResponse>, (StatusCode, String)> {
    let action = db
        .get_last_undoable_action()
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {e}"),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Nothing to undo".to_string()))?;

    let rolled_back = db
        .rollback_action(&action, None)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Undo failed: {e}")))?;

    Ok(Json(RollbackResponse {
        success: true,
        message: rolled_back.description.clone(),
        rolled_back_action: rolled_back,
    }))
}

pub async fn redo_last_handler(
    TenantDb(db): TenantDb,
) -> Result<Json<RollbackResponse>, (StatusCode, String)> {
    let action = db
        .get_last_redoable_action()
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {e}"),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Nothing to redo".to_string()))?;

    let rolled_back = db
        .rollback_action(&action, None)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Redo failed: {e}")))?;

    Ok(Json(RollbackResponse {
        success: true,
        message: rolled_back.description.clone(),
        rolled_back_action: rolled_back,
    }))
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the Axum application router.
///
/// Auth routes (`/auth/*`) and Swagger UI are enabled when `state.auth_state` is
/// `Some`.  Pass `None` to run without authentication (single-tenant dev / tests).
pub fn create_router(state: AppState) -> Router {
    let auth_state = state.auth_state.clone();

    let mut app = Router::new()
        .route("/version", get(version_handler))
        .route("/queue/add", post(add_to_queue_handler))
        .route("/search", post(search_handler))
        .route("/admin/allowed-domains", post(add_domain_handler))
        .route("/admin/allowed-domains", get(list_domains_handler))
        .route(
            "/admin/allowed-domains/{domain}",
            delete(delete_domain_handler),
        )
        .route(
            "/admin/crawled-pages/recent",
            get(recent_crawled_pages_handler),
        )
        .route(
            "/admin/settings/crawling-enabled",
            get(get_crawling_enabled_handler),
        )
        .route(
            "/admin/settings/crawling-enabled",
            put(set_crawling_enabled_handler),
        )
        .route("/admin/action-history/state", get(undo_redo_state_handler))
        .route("/admin/action-history/undo", post(undo_last_handler))
        .route("/admin/action-history/redo", post(redo_last_handler))
        .with_state(state);

    if let Some(auth) = auth_state {
        let auth_routes = auth_router().with_state(auth);
        app = app.nest("/auth", auth_routes).merge(
            SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", AuthApiDoc::openapi()),
        );
    }

    app.layer(CookieManagerLayer::new())
}
