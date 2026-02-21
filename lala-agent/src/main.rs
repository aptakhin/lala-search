// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use lala_agent::app::{create_router, AppState};
use lala_agent::models::agent::AgentMode;
use lala_agent::models::deployment::DeploymentMode;
use lala_agent::routes::AuthState;
use lala_agent::services::auth::AuthConfig;
use lala_agent::services::auth_db::AuthDbClient;
use lala_agent::services::db::CassandraClient;
use lala_agent::services::email::{EmailConfig, EmailService};
use lala_agent::services::queue_processor::QueueProcessor;
use lala_agent::services::search::SearchClient;
use lala_agent::services::storage::{S3Config, StorageClient};
use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let cassandra_hosts: Vec<String> = env::var("CASSANDRA_HOSTS")
        .expect("CASSANDRA_HOSTS environment variable must be set")
        .split(',')
        .map(|s| s.to_string())
        .collect();

    let cassandra_keyspace = env::var("CASSANDRA_KEYSPACE")
        .expect("CASSANDRA_KEYSPACE environment variable must be set");

    let cassandra_system_keyspace = env::var("CASSANDRA_SYSTEM_KEYSPACE")
        .expect("CASSANDRA_SYSTEM_KEYSPACE environment variable must be set");

    let agent_mode = AgentMode::from_env();
    let deployment_mode = DeploymentMode::from_env();

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
    let base_db = init_cassandra_client(&cassandra_hosts, &cassandra_keyspace).await;
    let search_client = init_search_client(&meilisearch_host, &meilisearch_index).await;
    let storage_client = init_storage_client().await;

    // Ensure the default tenant row exists in the system keyspace
    if let Err(e) = system_db.ensure_default_tenant().await {
        eprintln!("Failed to ensure default tenant in system keyspace: {}", e);
    }

    println!("Deployment mode: {}", deployment_mode);

    // Determine which tenant keyspaces the queue processor should handle.
    // In multi-tenant mode, CASSANDRA_TENANT_KEYSPACES lists all keyspaces
    // (comma-separated). Falls back to the single configured keyspace.
    let tenant_keyspaces = resolve_tenant_keyspaces(&cassandra_keyspace, deployment_mode);

    // Start one queue processor per tenant keyspace (if agent mode requires it)
    if agent_mode.should_process_queue() {
        let poll_interval = Duration::from_secs(poll_interval_secs);
        for ks in &tenant_keyspaces {
            let tenant_db = Arc::new(base_db.with_keyspace(ks));
            spawn_queue_processor(
                tenant_db,
                search_client.clone(),
                storage_client.clone(),
                user_agent.clone(),
                poll_interval,
            );
        }
        println!(
            "Queue processor(s) started for {} keyspace(s): {}",
            tenant_keyspaces.len(),
            tenant_keyspaces.join(", ")
        );
    }

    // Initialize auth state and build the HTTP app
    let auth_state = init_auth_state(system_db, &cassandra_keyspace).await;
    let state = AppState {
        db_client: base_db,
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

/// Resolve the list of Cassandra keyspaces the queue processor should watch.
///
/// In multi-tenant mode reads `CASSANDRA_TENANT_KEYSPACES` (comma-separated).
/// If that variable is absent or empty, falls back to `default_keyspace`.
fn resolve_tenant_keyspaces(default_keyspace: &str, mode: DeploymentMode) -> Vec<String> {
    if mode == DeploymentMode::MultiTenant {
        if let Ok(val) = env::var("CASSANDRA_TENANT_KEYSPACES") {
            let keyspaces: Vec<String> = val
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if !keyspaces.is_empty() {
                return keyspaces;
            }
        }
    }
    vec![default_keyspace.to_string()]
}

/// Spawn a background queue processor for one tenant's keyspace.
fn spawn_queue_processor(
    db_client: Arc<CassandraClient>,
    search_client: Option<Arc<SearchClient>>,
    storage_client: Option<Arc<StorageClient>>,
    user_agent: String,
    poll_interval: Duration,
) {
    let processor = match (&search_client, &storage_client) {
        (Some(search), Some(storage)) => QueueProcessor::with_all(
            db_client,
            search.clone(),
            storage.clone(),
            user_agent,
            poll_interval,
        ),
        (Some(search), None) => {
            QueueProcessor::with_search(db_client, search.clone(), user_agent, poll_interval)
        }
        (None, Some(storage)) => {
            QueueProcessor::with_storage(db_client, storage.clone(), user_agent, poll_interval)
        }
        (None, None) => QueueProcessor::new(db_client, user_agent, poll_interval),
    };

    tokio::spawn(async move {
        processor.start().await;
    });
}

/// Initialize Cassandra database client.
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
async fn init_auth_state(
    system_db: Arc<CassandraClient>,
    default_tenant_id: &str,
) -> Option<AuthState> {
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

    Some(AuthState::new(
        auth_db,
        email_service,
        auth_config,
        default_tenant_id.to_string(),
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
    use lala_agent::services::db::CassandraClient;
    use tower::ServiceExt;

    async fn create_test_app() -> axum::Router {
        let db_client = match CassandraClient::new(
            vec!["127.0.0.1:9042".to_string()],
            "lalasearch_test".to_string(),
        )
        .await
        {
            Ok(client) => Arc::new(client),
            Err(_) => Arc::new(
                CassandraClient::new(
                    vec!["127.0.0.1:9042".to_string()],
                    "lalasearch_default".to_string(),
                )
                .await
                .expect("Failed to connect to database"),
            ),
        };

        let state = AppState {
            db_client,
            search_client: None,
            deployment_mode: DeploymentMode::SingleTenant,
            auth_state: None,
        };
        create_router(state)
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
        let version_response: VersionResponse = serde_json::from_slice(&body).unwrap();

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

        assert_eq!(response_data.domains.len(), response_data.count);
    }

    #[tokio::test]
    #[ignore] // Requires Cassandra connection
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
    #[ignore] // Requires Cassandra connection
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
    #[ignore] // Requires Cassandra connection
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

        let _ = response_data.enabled;
    }

    #[tokio::test]
    #[ignore] // Requires Cassandra connection
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
