// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post, put},
    Json, Router,
};
use chrono::Utc;
use lala_agent::models::agent::AgentMode;
use lala_agent::models::db::CrawlQueueEntry;
use lala_agent::models::deployment::DeploymentMode;
use lala_agent::models::domain::{
    AddDomainRequest, AddDomainResponse, DeleteDomainResponse, DomainInfo, ListDomainsResponse,
};
use lala_agent::models::queue::{AddToQueueRequest, AddToQueueResponse};
use lala_agent::models::search::{SearchRequest, SearchResponse};
use lala_agent::models::settings::{CrawlingEnabledResponse, SetCrawlingEnabledRequest};
use lala_agent::models::version::VersionResponse;
use lala_agent::routes::{auth_router, AuthApiDoc, AuthState};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;
use lala_agent::services::auth::AuthConfig;
use lala_agent::services::auth_db::AuthDbClient;
use lala_agent::services::db::CassandraClient;
use lala_agent::services::email::{EmailConfig, EmailService};
use lala_agent::services::queue_processor::QueueProcessor;
use lala_agent::services::search::SearchClient;
use lala_agent::services::storage::{S3Config, StorageClient};
use scylla::frame::value::CqlTimestamp;
use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tower_cookies::CookieManagerLayer;

// Version is extracted from Cargo.toml at compile time via build.rs
// In CI/CD, the patch version can be overridden via LALA_PATCH_VERSION env var
const VERSION: &str = env!("LALA_VERSION");

#[derive(Clone)]
struct AppState {
    db_client: Arc<CassandraClient>,
    search_client: Option<Arc<SearchClient>>,
    deployment_mode: DeploymentMode,
}

struct QueueProcessorConfig {
    db_client: Arc<CassandraClient>,
    search_client: Option<Arc<SearchClient>>,
    storage_client: Option<Arc<StorageClient>>,
    user_agent: String,
    poll_interval_secs: u64,
}

async fn version_handler(State(state): State<AppState>) -> Json<VersionResponse> {
    Json(VersionResponse {
        agent: "lala-agent".to_string(),
        version: VERSION.to_string(),
        deployment_mode: state.deployment_mode.to_string(),
    })
}

async fn add_to_queue_handler(
    State(state): State<AppState>,
    Json(payload): Json<AddToQueueRequest>,
) -> Result<Json<AddToQueueResponse>, (StatusCode, String)> {
    // Parse and validate URL
    let parsed_url = url::Url::parse(&payload.url)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid URL: {}", e)))?;

    let domain = parsed_url
        .host_str()
        .ok_or((StatusCode::BAD_REQUEST, "URL has no host".to_string()))?
        .to_string();

    // Check if domain is allowed
    let is_allowed = state
        .db_client
        .is_domain_allowed(&domain)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to check domain allowlist: {}", e),
            )
        })?;

    if !is_allowed {
        return Err((
            StatusCode::FORBIDDEN,
            format!("Domain '{}' is not in the allowed domains list", domain),
        ));
    }

    // Create queue entry
    let now = Utc::now();
    let now_timestamp = CqlTimestamp(now.timestamp_millis());

    let entry = CrawlQueueEntry {
        priority: payload.priority,
        scheduled_at: now_timestamp,
        url: payload.url.clone(),
        domain: domain.clone(),
        last_attempt_at: None,
        attempt_count: 0,
        created_at: now_timestamp,
    };

    // Insert into database
    state
        .db_client
        .insert_queue_entry(&entry)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {}", e),
            )
        })?;

    Ok(Json(AddToQueueResponse {
        success: true,
        message: "URL added to crawl queue successfully".to_string(),
        url: payload.url,
        domain,
    }))
}

async fn search_handler(
    State(state): State<AppState>,
    Json(payload): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, (StatusCode, String)> {
    if let Some(search_client) = &state.search_client {
        search_client.search(payload).await.map(Json).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Search error: {}", e),
            )
        })
    } else {
        Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "Search service is not available".to_string(),
        ))
    }
}

async fn add_domain_handler(
    State(state): State<AppState>,
    Json(payload): Json<AddDomainRequest>,
) -> Result<Json<AddDomainResponse>, (StatusCode, String)> {
    // Validate domain format
    if payload.domain.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Domain cannot be empty".to_string(),
        ));
    }

    // Insert domain into database
    state
        .db_client
        .insert_allowed_domain(&payload.domain, "api", payload.notes.as_deref())
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {}", e),
            )
        })?;

    Ok(Json(AddDomainResponse {
        success: true,
        message: "Domain added to allowed list successfully".to_string(),
        domain: payload.domain,
    }))
}

async fn list_domains_handler(
    State(state): State<AppState>,
) -> Result<Json<ListDomainsResponse>, (StatusCode, String)> {
    let domains = state.db_client.list_allowed_domains().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
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

async fn delete_domain_handler(
    State(state): State<AppState>,
    Path(domain): Path<String>,
) -> Result<Json<DeleteDomainResponse>, (StatusCode, String)> {
    state
        .db_client
        .delete_allowed_domain(&domain)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {}", e),
            )
        })?;

    Ok(Json(DeleteDomainResponse {
        success: true,
        message: "Domain removed from allowed list successfully".to_string(),
        domain,
    }))
}

async fn get_crawling_enabled_handler(
    State(state): State<AppState>,
) -> Result<Json<CrawlingEnabledResponse>, (StatusCode, String)> {
    let enabled = state.db_client.is_crawling_enabled().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;

    Ok(Json(CrawlingEnabledResponse { enabled }))
}

async fn set_crawling_enabled_handler(
    State(state): State<AppState>,
    Json(payload): Json<SetCrawlingEnabledRequest>,
) -> Result<Json<CrawlingEnabledResponse>, (StatusCode, String)> {
    state
        .db_client
        .set_crawling_enabled(payload.enabled)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {}", e),
            )
        })?;

    Ok(Json(CrawlingEnabledResponse {
        enabled: payload.enabled,
    }))
}

#[tokio::main]
async fn main() {
    // Get configuration from environment variables
    let cassandra_hosts = env::var("CASSANDRA_HOSTS")
        .expect("CASSANDRA_HOSTS environment variable must be set")
        .split(',')
        .map(|s| s.to_string())
        .collect::<Vec<_>>();

    let cassandra_keyspace = env::var("CASSANDRA_KEYSPACE")
        .expect("CASSANDRA_KEYSPACE environment variable must be set");

    let agent_mode = AgentMode::from_env();
    let deployment_mode = DeploymentMode::from_env();

    let cassandra_system_keyspace = env::var("CASSANDRA_SYSTEM_KEYSPACE")
        .expect("CASSANDRA_SYSTEM_KEYSPACE environment variable must be set");

    let poll_interval_secs = env::var("QUEUE_POLL_INTERVAL_SECS")
        .expect("QUEUE_POLL_INTERVAL_SECS environment variable must be set")
        .parse::<u64>()
        .expect("QUEUE_POLL_INTERVAL_SECS must be a valid number");

    let user_agent = env::var("USER_AGENT").expect("USER_AGENT environment variable must be set");

    let meilisearch_host =
        env::var("MEILISEARCH_HOST").expect("MEILISEARCH_HOST environment variable must be set");

    let meilisearch_index =
        env::var("MEILISEARCH_INDEX").unwrap_or_else(|_| "documents".to_string());

    // Initialize database, search, and storage clients
    let system_db = init_cassandra_client(&cassandra_hosts, &cassandra_system_keyspace).await;
    let db_client = init_cassandra_client(&cassandra_hosts, &cassandra_keyspace).await;
    let search_client = init_search_client(&meilisearch_host, &meilisearch_index).await;
    let storage_client = init_storage_client().await;

    // Ensure the default tenant row exists in the system keyspace
    if let Err(e) = system_db.ensure_default_tenant().await {
        eprintln!("Failed to ensure default tenant in system keyspace: {}", e);
    }

    println!("Deployment mode: {}", deployment_mode);

    // Start queue processor if needed
    start_queue_processor_if_needed(
        agent_mode,
        QueueProcessorConfig {
            db_client: db_client.clone(),
            search_client: search_client.clone(),
            storage_client,
            user_agent,
            poll_interval_secs,
        },
    );

    // Start HTTP server
    start_http_server(db_client, system_db, search_client, deployment_mode).await;
}

/// Initialize Cassandra database client
async fn init_cassandra_client(hosts: &[String], keyspace: &str) -> Arc<CassandraClient> {
    match CassandraClient::new(hosts.to_vec(), keyspace.to_string()).await {
        Ok(client) => {
            println!(
                "Connected to Cassandra at {:?} using keyspace '{}'",
                hosts, keyspace
            );
            Arc::new(client)
        }
        Err(e) => {
            eprintln!("Failed to connect to Cassandra: {}", e);
            eprintln!("Continuing without database connection");
            Arc::new(
                CassandraClient::new(vec!["127.0.0.1:9042".to_string()], keyspace.to_string())
                    .await
                    .unwrap(),
            )
        }
    }
}

/// Initialize Meilisearch client
async fn init_search_client(host: &str, index_name: &str) -> Option<Arc<SearchClient>> {
    match SearchClient::new(host, index_name.to_string()).await {
        Ok(client) => {
            let client = Arc::new(client);
            if let Err(e) = client.init_index().await {
                eprintln!("Failed to initialize Meilisearch index: {}", e);
            }
            Some(client)
        }
        Err(e) => {
            eprintln!("Failed to connect to Meilisearch: {}", e);
            eprintln!("Continuing without search functionality");
            None
        }
    }
}

/// Initialize S3-compatible storage client
async fn init_storage_client() -> Option<Arc<StorageClient>> {
    match S3Config::from_env() {
        Ok(config) => match StorageClient::new(config).await {
            Ok(client) => Some(Arc::new(client)),
            Err(e) => {
                eprintln!("Failed to initialize S3 storage: {}", e);
                eprintln!("Continuing without content storage");
                None
            }
        },
        Err(_) => {
            println!("S3 storage not configured, skipping content storage");
            None
        }
    }
}

/// Start queue processor if agent mode requires it
fn start_queue_processor_if_needed(agent_mode: AgentMode, config: QueueProcessorConfig) {
    if !agent_mode.should_process_queue() {
        return;
    }

    let poll_interval = Duration::from_secs(config.poll_interval_secs);
    let processor = match (&config.search_client, &config.storage_client) {
        (Some(search), Some(storage)) => QueueProcessor::with_all(
            config.db_client,
            search.clone(),
            storage.clone(),
            config.user_agent,
            poll_interval,
        ),
        (Some(search), None) => QueueProcessor::with_search(
            config.db_client,
            search.clone(),
            config.user_agent,
            poll_interval,
        ),
        (None, Some(storage)) => QueueProcessor::with_storage(
            config.db_client,
            storage.clone(),
            config.user_agent,
            poll_interval,
        ),
        (None, None) => QueueProcessor::new(config.db_client, config.user_agent, poll_interval),
    };

    tokio::spawn(async move {
        processor.start().await;
    });

    println!("Queue processor started in background");
}

/// Start the HTTP server
async fn start_http_server(
    db_client: Arc<CassandraClient>,
    system_db: Arc<CassandraClient>,
    search_client: Option<Arc<SearchClient>>,
    deployment_mode: DeploymentMode,
) {
    let state = AppState {
        db_client,
        search_client,
        deployment_mode,
    };

    // Initialize auth services (optional - only if email is configured)
    let auth_state = init_auth_state(system_db).await;

    let app = create_app(state, auth_state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    println!("lala-agent v{} listening on {}", VERSION, addr);

    axum::serve(listener, app).await.unwrap();
}

/// Initialize auth state if email service is configured.
async fn init_auth_state(system_db: Arc<CassandraClient>) -> Option<AuthState> {
    // Check if email service is configured
    let email_config = match EmailConfig::from_env() {
        Ok(config) => config,
        Err(e) => {
            println!(
                "Email service not configured ({}), authentication disabled",
                e
            );
            return None;
        }
    };

    let email_service = match EmailService::new(email_config) {
        Ok(service) => {
            println!("Email service configured, authentication enabled");
            service
        }
        Err(e) => {
            println!(
                "Failed to initialize email service ({}), authentication disabled",
                e
            );
            return None;
        }
    };

    let keyspace =
        env::var("CASSANDRA_SYSTEM_KEYSPACE").unwrap_or_else(|_| "lalasearch_system".to_string());

    let auth_db = AuthDbClient::new(system_db.session(), keyspace);
    let auth_config = AuthConfig::from_env();
    let default_tenant_id =
        env::var("CASSANDRA_KEYSPACE").unwrap_or_else(|_| "lalasearch_default".to_string());

    Some(AuthState::new(
        auth_db,
        email_service,
        auth_config,
        default_tenant_id,
    ))
}

fn create_app(state: AppState, auth_state: Option<AuthState>) -> Router {
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

    // Add auth routes and Swagger UI if configured
    if let Some(auth_state) = auth_state {
        let auth_routes = auth_router().with_state(auth_state);
        app = app
            .nest("/auth", auth_routes)
            .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", AuthApiDoc::openapi()));
    }

    // Add cookie layer for session management
    app.layer(CookieManagerLayer::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use axum::http::StatusCode;
    use tower::ServiceExt;

    async fn create_test_app() -> Router {
        // For unit tests, try to connect to test database, but fallback to main keyspace
        // Tests that require database should be marked with #[ignore]
        let db_client = match CassandraClient::new(
            vec!["127.0.0.1:9042".to_string()],
            "lalasearch_test".to_string(),
        )
        .await
        {
            Ok(client) => Arc::new(client),
            Err(_) => {
                // If test database is not available, use default keyspace
                Arc::new(
                    CassandraClient::new(
                        vec!["127.0.0.1:9042".to_string()],
                        "lalasearch_default".to_string(),
                    )
                    .await
                    .expect("Failed to connect to database"),
                )
            }
        };

        let state = AppState {
            db_client,
            search_client: None,
            deployment_mode: DeploymentMode::SingleTenant,
        };
        create_app(state, None)
    }

    #[tokio::test]
    #[ignore] // Requires Cassandra
    async fn test_version_endpoint_response() {
        let app = create_test_app().await;

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/version")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Check status code
        assert_eq!(response.status(), StatusCode::OK);

        // Check content-type header
        let content_type = response.headers().get("content-type").unwrap();
        assert_eq!(content_type, "application/json");

        // Parse and validate response structure
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        let version_response: VersionResponse = serde_json::from_str(&body_str).unwrap();

        assert_eq!(version_response.agent, "lala-agent");
        assert_eq!(version_response.version, VERSION);
    }

    #[tokio::test]
    #[ignore] // Requires Cassandra
    async fn test_version_follows_semver_format() {
        let app = create_test_app().await;

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/version")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body.to_vec()).unwrap();

        let version_response: VersionResponse = serde_json::from_str(&body_str).unwrap();

        // Check semver format: MAJOR.MINOR.PATCH
        let parts: Vec<&str> = version_response.version.split('.').collect();
        assert_eq!(parts.len(), 3);
        assert!(parts[0].parse::<u32>().is_ok());
        assert!(parts[1].parse::<u32>().is_ok());
        assert!(parts[2].parse::<u32>().is_ok());
    }

    #[tokio::test]
    #[ignore] // Requires Cassandra
    async fn test_invalid_route_returns_404() {
        let app = create_test_app().await;

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/invalid")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    #[ignore] // Requires Cassandra
    async fn test_concurrent_requests_succeed() {
        let app = create_test_app().await;

        let handles: Vec<_> = (0..10)
            .map(|_| {
                let app_clone = app.clone();
                tokio::spawn(async move {
                    let response = app_clone
                        .oneshot(
                            Request::builder()
                                .uri("/version")
                                .body(Body::empty())
                                .unwrap(),
                        )
                        .await
                        .unwrap();
                    response.status()
                })
            })
            .collect();

        for handle in handles {
            let status = handle.await.unwrap();
            assert_eq!(status, StatusCode::OK);
        }
    }

    #[tokio::test]
    #[ignore] // Requires Cassandra connection
    async fn test_add_to_queue_valid_url() {
        let app = create_test_app().await;

        let request_body = AddToQueueRequest {
            url: "https://en.wikipedia.org/wiki/Main_Page".to_string(),
            priority: 1,
        };

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/queue/add")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&request_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let response_data: AddToQueueResponse = serde_json::from_slice(&body).unwrap();

        assert!(response_data.success);
        assert_eq!(response_data.url, request_body.url);
        assert_eq!(response_data.domain, "en.wikipedia.org");
    }

    #[tokio::test]
    #[ignore] // Requires Cassandra connection
    async fn test_add_to_queue_invalid_url() {
        let app = create_test_app().await;

        let request_body = AddToQueueRequest {
            url: "not-a-valid-url".to_string(),
            priority: 1,
        };

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/queue/add")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&request_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    #[ignore] // Requires Cassandra connection
    async fn test_add_to_queue_domain_not_allowed() {
        let app = create_test_app().await;

        // Try to add a URL from a domain that's not in allowed_domains
        let request_body = AddToQueueRequest {
            url: "https://example.com/page".to_string(),
            priority: 1,
        };

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/queue/add")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&request_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body.to_vec()).unwrap();

        assert!(body_str.contains("not in the allowed domains list"));
        assert!(body_str.contains("example.com"));
    }

    #[tokio::test]
    #[ignore] // Requires Cassandra connection
    async fn test_add_domain_success() {
        let app = create_test_app().await;

        let test_domain = format!("test-add-{}.example.com", Utc::now().timestamp_millis());
        let request_body = AddDomainRequest {
            domain: test_domain.clone(),
            notes: Some("Test domain for smoke test".to_string()),
        };

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/allowed-domains")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&request_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let response_data: AddDomainResponse = serde_json::from_slice(&body).unwrap();

        assert!(response_data.success);
        assert_eq!(response_data.domain, test_domain);
        assert!(response_data
            .message
            .contains("Domain added to allowed list successfully"));

        // Cleanup: delete the test domain
        let _cleanup = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/admin/allowed-domains/{}", test_domain))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
    }

    #[tokio::test]
    #[ignore] // Requires Cassandra connection
    async fn test_add_domain_empty_domain() {
        let app = create_test_app().await;

        let request_body = AddDomainRequest {
            domain: "".to_string(),
            notes: None,
        };

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/allowed-domains")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&request_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body.to_vec()).unwrap();

        assert!(body_str.contains("Domain cannot be empty"));
    }

    #[tokio::test]
    #[ignore] // Requires Cassandra connection
    async fn test_list_domains_success() {
        let app = create_test_app().await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/admin/allowed-domains")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let response_data: ListDomainsResponse = serde_json::from_slice(&body).unwrap();

        // Should return a list (may be empty or have existing domains)
        assert_eq!(response_data.domains.len(), response_data.count);
    }

    #[tokio::test]
    #[ignore] // Requires Cassandra connection
    async fn test_list_domains_includes_added_domain() {
        let app = create_test_app().await;

        // Add a test domain first
        let test_domain = format!("test-list-{}.example.com", Utc::now().timestamp_millis());
        let add_request = AddDomainRequest {
            domain: test_domain.clone(),
            notes: Some("Test for list endpoint".to_string()),
        };

        let _add_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/allowed-domains")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&add_request).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // List domains and verify our test domain is included
        let list_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/admin/allowed-domains")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(list_response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(list_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let response_data: ListDomainsResponse = serde_json::from_slice(&body).unwrap();

        let found = response_data
            .domains
            .iter()
            .any(|d| d.domain == test_domain);
        assert!(found, "Added domain should appear in list");

        // Verify domain info structure
        if let Some(domain_info) = response_data
            .domains
            .iter()
            .find(|d| d.domain == test_domain)
        {
            assert_eq!(domain_info.added_by, Some("api".to_string()));
            assert_eq!(
                domain_info.notes,
                Some("Test for list endpoint".to_string())
            );
            assert!(domain_info.added_at.is_some());
        }

        // Cleanup
        let _cleanup = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/admin/allowed-domains/{}", test_domain))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
    }

    #[tokio::test]
    #[ignore] // Requires Cassandra connection
    async fn test_delete_domain_success() {
        let app = create_test_app().await;

        // Add a test domain first
        let test_domain = format!("test-delete-{}.example.com", Utc::now().timestamp_millis());
        let add_request = AddDomainRequest {
            domain: test_domain.clone(),
            notes: Some("Test domain for deletion".to_string()),
        };

        let _add_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/allowed-domains")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&add_request).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Delete the domain
        let delete_response = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/admin/allowed-domains/{}", test_domain))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(delete_response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(delete_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let response_data: DeleteDomainResponse = serde_json::from_slice(&body).unwrap();

        assert!(response_data.success);
        assert_eq!(response_data.domain, test_domain);
        assert!(response_data
            .message
            .contains("Domain removed from allowed list successfully"));
    }

    #[tokio::test]
    #[ignore] // Requires Cassandra connection
    async fn test_delete_nonexistent_domain() {
        let app = create_test_app().await;

        let nonexistent_domain =
            format!("nonexistent-{}.example.com", Utc::now().timestamp_millis());

        // Deleting a non-existent domain should still succeed (idempotent operation)
        let response = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/admin/allowed-domains/{}", nonexistent_domain))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let response_data: DeleteDomainResponse = serde_json::from_slice(&body).unwrap();

        assert!(response_data.success);
        assert_eq!(response_data.domain, nonexistent_domain);
    }

    #[tokio::test]
    #[ignore] // Requires Cassandra connection
    async fn test_get_crawling_enabled() {
        let app = create_test_app().await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/admin/settings/crawling-enabled")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let response_data: CrawlingEnabledResponse = serde_json::from_slice(&body).unwrap();

        // Should return a boolean value (either true or false)
        // We just verify the response structure is valid
        let _ = response_data.enabled;
    }

    #[tokio::test]
    #[ignore] // Requires Cassandra connection
    async fn test_set_crawling_enabled() {
        let app = create_test_app().await;

        // Set crawling to false
        let request_body = SetCrawlingEnabledRequest { enabled: false };

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/admin/settings/crawling-enabled")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&request_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let response_data: CrawlingEnabledResponse = serde_json::from_slice(&body).unwrap();
        assert!(!response_data.enabled);

        // Verify it persisted by reading it back
        let get_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/admin/settings/crawling-enabled")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(get_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let response_data: CrawlingEnabledResponse = serde_json::from_slice(&body).unwrap();
        assert!(!response_data.enabled, "Crawling should be disabled");

        // Set crawling back to true
        let request_body = SetCrawlingEnabledRequest { enabled: true };

        let response = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/admin/settings/crawling-enabled")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&request_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let response_data: CrawlingEnabledResponse = serde_json::from_slice(&body).unwrap();
        assert!(response_data.enabled);
    }
}
