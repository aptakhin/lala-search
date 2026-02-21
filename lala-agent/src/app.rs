// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

//! Application state, per-request tenant DB resolution, route handlers, and router
//! construction.
//!
//! This module is `pub` so that integration tests can build a test router directly
//! without starting the full binary.

use crate::models::db::CrawlQueueEntry;
use crate::models::deployment::DeploymentMode;
use crate::models::domain::{
    AddDomainRequest, AddDomainResponse, DeleteDomainResponse, DomainInfo, ListDomainsResponse,
};
use crate::models::queue::{AddToQueueRequest, AddToQueueResponse};
use crate::models::search::{SearchRequest, SearchResponse};
use crate::models::settings::{CrawlingEnabledResponse, SetCrawlingEnabledRequest};
use crate::models::version::VersionResponse;
use crate::routes::{auth_router, AuthApiDoc, AuthState};
use crate::services::db::CassandraClient;
use crate::services::search::SearchClient;
use axum::{
    extract::{FromRequestParts, Path, State},
    http::{request::Parts, StatusCode},
    routing::{delete, get, post, put},
    Json, Router,
};
use chrono::Utc;
use scylla::frame::value::CqlTimestamp;
use std::sync::Arc;
use tower_cookies::{CookieManagerLayer, Cookies};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

/// Application version extracted from `Cargo.toml` at compile time.
/// The patch segment can be overridden via `LALA_PATCH_VERSION` (see `build.rs`).
pub const VERSION: &str = env!("LALA_VERSION");

// ---------------------------------------------------------------------------
// Application state
// ---------------------------------------------------------------------------

/// Shared application state injected into every route handler via `State<AppState>`.
#[derive(Clone)]
pub struct AppState {
    /// Base Cassandra client.  Used as-is in single-tenant mode; used as the
    /// connection-pool source for `with_keyspace()` in multi-tenant mode.
    pub db_client: Arc<CassandraClient>,
    pub search_client: Option<Arc<SearchClient>>,
    pub deployment_mode: DeploymentMode,
    /// Required in multi-tenant mode: validates session cookies and resolves
    /// the authenticated user's tenant keyspace.
    pub auth_state: Option<AuthState>,
}

// ---------------------------------------------------------------------------
// Per-request tenant DB extractor
// ---------------------------------------------------------------------------

/// Axum extractor that resolves the Cassandra client to use for the current request.
///
/// * **Single-tenant mode**: returns `state.db_client` directly; no authentication needed.
/// * **Multi-tenant mode**: reads the `lala_session` cookie, validates it, extracts
///   `auth_user.tenant_id` (which is the Cassandra keyspace name), and returns
///   `state.db_client.with_keyspace(tenant_id)`.
pub struct TenantDb(pub Arc<CassandraClient>);

impl FromRequestParts<AppState> for TenantDb {
    type Rejection = (StatusCode, String);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if state.deployment_mode == DeploymentMode::SingleTenant {
            return Ok(TenantDb(state.db_client.clone()));
        }
        resolve_multi_tenant_db(parts, state).await.map(TenantDb)
    }
}

/// Resolve the tenant-scoped DB client for a multi-tenant request.
async fn resolve_multi_tenant_db(
    parts: &mut Parts,
    state: &AppState,
) -> Result<Arc<CassandraClient>, (StatusCode, String)> {
    let auth_state = state.auth_state.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "Auth service not configured for multi-tenant mode".to_string(),
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
                "Authentication required for multi-tenant access".to_string(),
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

    Ok(Arc::new(
        state.db_client.with_keyspace(&auth_user.tenant_id),
    ))
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
    let now_ts = CqlTimestamp(now.timestamp_millis());
    let entry = CrawlQueueEntry {
        priority: payload.priority,
        scheduled_at: now_ts,
        url: payload.url.clone(),
        domain: domain.clone(),
        last_attempt_at: None,
        attempt_count: 0,
        created_at: now_ts,
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

pub async fn search_handler(
    State(state): State<AppState>,
    Json(payload): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, (StatusCode, String)> {
    let search_client = state.search_client.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "Search service is not available".to_string(),
        )
    })?;

    search_client.search(payload).await.map(Json).map_err(|e| {
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

    Ok(Json(AddDomainResponse {
        success: true,
        message: "Domain added to allowed list successfully".to_string(),
        domain: payload.domain,
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
    db.delete_allowed_domain(&domain).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {e}"),
        )
    })?;

    Ok(Json(DeleteDomainResponse {
        success: true,
        message: "Domain removed from allowed list successfully".to_string(),
        domain,
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

    Ok(Json(CrawlingEnabledResponse { enabled }))
}

pub async fn set_crawling_enabled_handler(
    TenantDb(db): TenantDb,
    Json(payload): Json<SetCrawlingEnabledRequest>,
) -> Result<Json<CrawlingEnabledResponse>, (StatusCode, String)> {
    db.set_crawling_enabled(payload.enabled)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {e}"),
            )
        })?;

    Ok(Json(CrawlingEnabledResponse {
        enabled: payload.enabled,
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
            "/admin/settings/crawling-enabled",
            get(get_crawling_enabled_handler),
        )
        .route(
            "/admin/settings/crawling-enabled",
            put(set_crawling_enabled_handler),
        )
        .with_state(state);

    if let Some(auth) = auth_state {
        let auth_routes = auth_router().with_state(auth);
        app = app.nest("/auth", auth_routes).merge(
            SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", AuthApiDoc::openapi()),
        );
    }

    app.layer(CookieManagerLayer::new())
}
