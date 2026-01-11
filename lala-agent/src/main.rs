// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use axum::{Json, Router, routing::get};
use lala_agent::models::version::VersionResponse;
use std::net::SocketAddr;

// Version is extracted from Cargo.toml at compile time via build.rs
// In CI/CD, the patch version can be overridden via LALA_PATCH_VERSION env var
const VERSION: &str = env!("LALA_VERSION");

async fn version_handler() -> Json<VersionResponse> {
    Json(VersionResponse {
        agent: "lala-agent".to_string(),
        version: VERSION.to_string(),
    })
}

#[tokio::main]
async fn main() {
    let app = create_app();

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    println!("lala-agent v{} listening on {}", VERSION, addr);

    axum::serve(listener, app).await.unwrap();
}

fn create_app() -> Router {
    Router::new().route("/version", get(version_handler))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use axum::http::StatusCode;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_version_endpoint_response() {
        let app = create_app();

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
        let app = create_app();

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
        let app = create_app();

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
        let app = create_app();

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
}
