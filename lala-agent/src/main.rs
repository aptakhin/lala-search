// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use lala_agent::models::agent::AgentMode;
use lala_agent::models::db::CrawlQueueEntry;
use lala_agent::models::queue::{AddToQueueRequest, AddToQueueResponse};
use lala_agent::models::search::{SearchRequest, SearchResponse};
use lala_agent::models::version::VersionResponse;
use lala_agent::services::db::CassandraClient;
use lala_agent::services::queue_processor::QueueProcessor;
use lala_agent::services::search::SearchClient;
use scylla::frame::value::CqlTimestamp;
use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

// Version is extracted from Cargo.toml at compile time via build.rs
// In CI/CD, the patch version can be overridden via LALA_PATCH_VERSION env var
const VERSION: &str = env!("LALA_VERSION");

#[derive(Clone)]
struct AppState {
    db_client: Arc<CassandraClient>,
    search_client: Option<Arc<SearchClient>>,
}

async fn version_handler() -> Json<VersionResponse> {
    Json(VersionResponse {
        agent: "lala-agent".to_string(),
        version: VERSION.to_string(),
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

#[tokio::main]
async fn main() {
    // Get configuration from environment variables
    // Support both CASSANDRA_HOSTS and legacy SCYLLA_HOSTS for backward compatibility
    let cassandra_hosts = env::var("CASSANDRA_HOSTS")
        .or_else(|_| env::var("SCYLLA_HOSTS"))
        .expect("CASSANDRA_HOSTS or SCYLLA_HOSTS environment variable must be set")
        .split(',')
        .map(|s| s.to_string())
        .collect::<Vec<_>>();

    let cassandra_keyspace = env::var("CASSANDRA_KEYSPACE")
        .or_else(|_| env::var("SCYLLA_KEYSPACE"))
        .expect("CASSANDRA_KEYSPACE or SCYLLA_KEYSPACE environment variable must be set");

    let agent_mode = AgentMode::from_env();

    let poll_interval_secs = env::var("QUEUE_POLL_INTERVAL_SECS")
        .expect("QUEUE_POLL_INTERVAL_SECS environment variable must be set")
        .parse::<u64>()
        .expect("QUEUE_POLL_INTERVAL_SECS must be a valid number");

    let user_agent = env::var("USER_AGENT").expect("USER_AGENT environment variable must be set");

    let meilisearch_host =
        env::var("MEILISEARCH_HOST").expect("MEILISEARCH_HOST environment variable must be set");

    // Initialize Cassandra client
    let db_client =
        match CassandraClient::new(cassandra_hosts.clone(), cassandra_keyspace.clone()).await {
            Ok(client) => {
                println!("Connected to Cassandra at {:?}", cassandra_hosts);
                Arc::new(client)
            }
            Err(e) => {
                eprintln!("Failed to connect to Cassandra: {}", e);
                eprintln!("Continuing without database connection");
                // In production, you might want to exit here
                // For now, we'll continue to allow the HTTP server to run
                Arc::new(
                    CassandraClient::new(vec!["127.0.0.1:9042".to_string()], cassandra_keyspace)
                        .await
                        .unwrap(),
                )
            }
        };

    // Initialize Meilisearch client
    let search_client = match SearchClient::new(&meilisearch_host).await {
        Ok(client) => {
            let client = Arc::new(client);
            // Initialize the documents index
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
    };

    // Start queue processor if agent mode should process queue
    if agent_mode.should_process_queue() {
        let processor = if let Some(ref search_client) = search_client {
            QueueProcessor::with_search(
                db_client.clone(),
                search_client.clone(),
                user_agent,
                Duration::from_secs(poll_interval_secs),
            )
        } else {
            QueueProcessor::new(
                db_client.clone(),
                user_agent,
                Duration::from_secs(poll_interval_secs),
            )
        };

        tokio::spawn(async move {
            processor.start().await;
        });

        println!("Queue processor started in background");
    }

    let state = AppState {
        db_client: db_client.clone(),
        search_client,
    };

    let app = create_app(state);

    // Bind to 0.0.0.0 to accept connections from any network interface (required for Docker)
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    println!("lala-agent v{} listening on {}", VERSION, addr);

    axum::serve(listener, app).await.unwrap();
}

fn create_app(state: AppState) -> Router {
    Router::new()
        .route("/version", get(version_handler))
        .route("/queue/add", post(add_to_queue_handler))
        .route("/search", post(search_handler))
        .with_state(state)
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
                // If test database is not available, use main keyspace
                Arc::new(
                    CassandraClient::new(
                        vec!["127.0.0.1:9042".to_string()],
                        "lalasearch".to_string(),
                    )
                    .await
                    .expect("Failed to connect to database"),
                )
            }
        };

        let state = AppState {
            db_client,
            search_client: None,
        };
        create_app(state)
    }

    #[tokio::test]
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
}
