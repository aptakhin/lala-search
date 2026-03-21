// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use clap::{Parser, Subcommand};
use lala_agent::app::{create_router, AppState};
use lala_agent::models::agent::AgentMode;
use lala_agent::models::deployment::DeploymentMode;
use lala_agent::routes::AuthState;
use lala_agent::services::auth::AuthConfig;
use lala_agent::services::auth_db::AuthDbClient;
use lala_agent::services::db::DbClient;
use lala_agent::services::email::{EmailConfig, EmailService};
use lala_agent::services::queue_processor::{QueueConfig, QueueProcessor};
use lala_agent::services::search::SearchClient;
use lala_agent::services::storage::{S3Config, StorageClient};
use sqlx::postgres::PgPool;
use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "lala-agent", about = "LalaSearch web crawler and search agent")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run database migrations to the latest version
    Migrate,
    /// Start the HTTP server (default)
    Serve,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command.unwrap_or(Command::Serve) {
        Command::Migrate => run_migrate().await,
        Command::Serve => run_serve().await,
    }
}

/// Run database migrations and exit.
async fn run_migrate() {
    let database_url =
        env::var("DATABASE_URL").expect("DATABASE_URL environment variable must be set");

    println!("[MIGRATE] Connecting to database...");
    let pool = PgPool::connect(&database_url)
        .await
        .unwrap_or_else(|e| panic!("Failed to connect to PostgreSQL at {}: {}", database_url, e));

    println!("[MIGRATE] Running migrations...");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .unwrap_or_else(|e| panic!("Migration failed: {:#}", e));

    println!("[MIGRATE] All migrations applied successfully.");
}

/// Start the HTTP server.
async fn run_serve() {
    let database_url =
        env::var("DATABASE_URL").expect("DATABASE_URL environment variable must be set");
    let default_tenant_id: Uuid = env::var("DEFAULT_TENANT_ID")
        .unwrap_or_else(|_| "00000000-0000-0000-0000-000000000001".to_string())
        .parse()
        .expect("DEFAULT_TENANT_ID must be a valid UUID");
    let agent_mode = AgentMode::from_env();
    let deployment_mode = DeploymentMode::from_env();
    let meilisearch_host =
        env::var("MEILISEARCH_HOST").expect("MEILISEARCH_HOST environment variable must be set");
    let meilisearch_index =
        env::var("MEILISEARCH_INDEX").unwrap_or_else(|_| "documents".to_string());

    let pool = init_db_pool(&database_url).await;
    let db_client = Arc::new(DbClient::new(pool.clone(), default_tenant_id));
    let search_client = init_search_client(&meilisearch_host, &meilisearch_index).await;
    let storage_client = init_storage_client().await;

    let tenant_name = env::var("TENANT_NAME").unwrap_or_else(|_| "My Organization".to_string());
    if let Err(e) = db_client
        .ensure_default_tenant(default_tenant_id, &tenant_name)
        .await
    {
        eprintln!("Failed to ensure default tenant: {}", e);
    }

    println!("Deployment mode: {}", deployment_mode);

    let tenant_ids = resolve_tenant_ids(&db_client, default_tenant_id, deployment_mode).await;
    if agent_mode.should_process_queue() {
        start_queue_processors(
            &db_client,
            &search_client,
            &storage_client,
            &tenant_ids,
            deployment_mode,
        );
    }

    let auth_state = init_auth_state(pool, default_tenant_id).await;
    let state = AppState {
        db_client,
        search_client,
        deployment_mode,
        auth_state,
    };
    let app = create_router(state);
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!(
        "lala-agent v{} listening on {}",
        lala_agent::app::VERSION,
        addr
    );
    axum::serve(listener, app).await.unwrap();
}

/// Resolve the list of tenant IDs the queue processor should handle.
///
/// In multi-tenant mode, queries the tenants table for all active tenants.
/// Falls back to `default_tenant_id` if the query fails or returns no rows.
async fn resolve_tenant_ids(
    db_client: &DbClient,
    default_tenant_id: Uuid,
    mode: DeploymentMode,
) -> Vec<Uuid> {
    if mode != DeploymentMode::MultiTenant {
        return vec![default_tenant_id];
    }

    match db_client.list_tenant_ids().await {
        Ok(ids) if !ids.is_empty() => {
            let id_list: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
            println!(
                "Scheduler: found {} valid tenant(s): {}",
                ids.len(),
                id_list.join(", ")
            );
            ids
        }
        Ok(_) => {
            println!(
                "Scheduler: no tenants found, using default: {}",
                default_tenant_id
            );
            vec![default_tenant_id]
        }
        Err(e) => {
            eprintln!(
                "Scheduler: failed to list tenants ({}), using default: {}",
                e, default_tenant_id
            );
            vec![default_tenant_id]
        }
    }
}

/// Start queue processors for all tenants.
fn start_queue_processors(
    db_client: &Arc<DbClient>,
    search_client: &Option<Arc<SearchClient>>,
    storage_client: &Option<Arc<StorageClient>>,
    tenant_ids: &[Uuid],
    deployment_mode: DeploymentMode,
) {
    let user_agent = env::var("USER_AGENT").expect("USER_AGENT environment variable must be set");
    let poll_interval_secs: u64 = env::var("QUEUE_POLL_INTERVAL_SECS")
        .expect("QUEUE_POLL_INTERVAL_SECS environment variable must be set")
        .parse()
        .expect("QUEUE_POLL_INTERVAL_SECS must be a valid number");
    let poll_interval = Duration::from_secs(poll_interval_secs);

    for &tid in tenant_ids {
        let tenant_db = Arc::new(db_client.with_tenant(tid));
        let tenant_id_str = if deployment_mode == DeploymentMode::MultiTenant {
            Some(tid.to_string())
        } else {
            None
        };
        spawn_queue_processor(
            tenant_db,
            search_client,
            storage_client,
            QueueConfig {
                user_agent: user_agent.to_string(),
                poll_interval,
                tenant_id: tenant_id_str,
            },
        );
    }
    let tenant_list: Vec<String> = tenant_ids.iter().map(|id| id.to_string()).collect();
    println!(
        "Queue processor(s) started for {} tenant(s): {}",
        tenant_ids.len(),
        tenant_list.join(", ")
    );
}

/// Spawn a background queue processor for one tenant.
fn spawn_queue_processor(
    db_client: Arc<DbClient>,
    search_client: &Option<Arc<SearchClient>>,
    storage_client: &Option<Arc<StorageClient>>,
    config: QueueConfig,
) {
    let processor = match (search_client, storage_client) {
        (Some(search), Some(storage)) => {
            QueueProcessor::with_all(db_client, search.clone(), storage.clone(), config)
        }
        (Some(search), None) => QueueProcessor::with_search(db_client, search.clone(), config),
        (None, Some(storage)) => QueueProcessor::with_storage(db_client, storage.clone(), config),
        (None, None) => QueueProcessor::new(db_client, config),
    };

    tokio::spawn(async move {
        processor.start().await;
    });
}

/// Initialize PostgreSQL connection pool.
async fn init_db_pool(database_url: &str) -> PgPool {
    PgPool::connect(database_url)
        .await
        .unwrap_or_else(|e| panic!("Failed to connect to PostgreSQL at {}: {}", database_url, e))
}

/// Initialize Meilisearch client.
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

/// Initialize S3-compatible storage client.
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

/// Initialize auth state if email is configured.
async fn init_auth_state(pool: PgPool, default_tenant_id: Uuid) -> Option<AuthState> {
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

    let auth_db = AuthDbClient::new(pool);
    let auth_config = AuthConfig::from_env();

    Some(AuthState::new(
        auth_db,
        email_service,
        auth_config,
        default_tenant_id,
    ))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use axum::http::StatusCode;
    use lala_agent::app::{create_router, AppState, VERSION};
    use lala_agent::models::deployment::DeploymentMode;
    use lala_agent::models::domain::{
        AddDomainRequest, AddDomainResponse, DeleteDomainResponse, ListDomainsResponse,
    };
    use lala_agent::models::queue::{AddToQueueRequest, AddToQueueResponse};
    use lala_agent::models::settings::{CrawlingEnabledResponse, SetCrawlingEnabledRequest};
    use lala_agent::models::version::VersionResponse;
    use lala_agent::services::db::DbClient;
    use tower::ServiceExt;

    async fn create_test_app() -> axum::Router {
        let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgres://lalasearch:lalasearch@127.0.0.1:5432/lalasearch".to_string()
        });

        let pool = PgPool::connect(&database_url)
            .await
            .expect("Failed to connect to PostgreSQL");

        let default_tenant_id: Uuid = env::var("DEFAULT_TENANT_ID")
            .unwrap_or_else(|_| "00000000-0000-0000-0000-000000000001".to_string())
            .parse()
            .expect("DEFAULT_TENANT_ID must be a valid UUID");

        let db_client = Arc::new(DbClient::new(pool, default_tenant_id));

        let state = AppState {
            db_client,
            search_client: None,
            deployment_mode: DeploymentMode::SingleTenant,
            auth_state: None,
        };
        create_router(state)
    }

    #[tokio::test]
    #[ignore] // Requires PostgreSQL
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

        assert_eq!(response.status(), StatusCode::OK);

        let content_type = response.headers().get("content-type").unwrap();
        assert_eq!(content_type, "application/json");

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let version_response: VersionResponse = serde_json::from_slice(&body).unwrap();

        assert_eq!(version_response.agent, "lala-agent");
        assert_eq!(version_response.version, VERSION);
    }

    #[tokio::test]
    #[ignore] // Requires PostgreSQL
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
        let version_response: VersionResponse = serde_json::from_slice(&body).unwrap();

        let parts: Vec<&str> = version_response.version.split('.').collect();
        assert_eq!(parts.len(), 3);
        assert!(parts[0].parse::<u32>().is_ok());
        assert!(parts[1].parse::<u32>().is_ok());
        assert!(parts[2].parse::<u32>().is_ok());
    }

    #[tokio::test]
    #[ignore] // Requires PostgreSQL
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
    #[ignore] // Requires PostgreSQL
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
    #[ignore] // Requires PostgreSQL
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
    #[ignore] // Requires PostgreSQL
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
    #[ignore] // Requires PostgreSQL
    async fn test_add_to_queue_domain_not_allowed() {
        let app = create_test_app().await;

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
    #[ignore] // Requires PostgreSQL
    async fn test_add_domain_success() {
        use chrono::Utc;
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
    #[ignore] // Requires PostgreSQL
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
    #[ignore] // Requires PostgreSQL
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

        assert_eq!(response_data.domains.len(), response_data.count);
    }

    #[tokio::test]
    #[ignore] // Requires PostgreSQL
    async fn test_list_domains_includes_added_domain() {
        use chrono::Utc;
        let app = create_test_app().await;

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
    #[ignore] // Requires PostgreSQL
    async fn test_delete_domain_success() {
        use chrono::Utc;
        let app = create_test_app().await;

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
    #[ignore] // Requires PostgreSQL
    async fn test_delete_nonexistent_domain() {
        use chrono::Utc;
        let app = create_test_app().await;

        let nonexistent_domain =
            format!("nonexistent-{}.example.com", Utc::now().timestamp_millis());

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
    #[ignore] // Requires PostgreSQL
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

        let _ = response_data.enabled;
    }

    #[tokio::test]
    #[ignore] // Requires PostgreSQL
    async fn test_set_crawling_enabled() {
        let app = create_test_app().await;

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
