// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

//! Integration tests for the database migration system.
//!
//! These tests verify that migrations apply cleanly to a fresh database
//! and that the resulting schema contains the expected tables.

use sqlx::postgres::PgPool;
use sqlx::Row;
use std::env;

/// Connect to the test database.
async fn test_pool() -> PgPool {
    let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://lalasearch:lalasearch@127.0.0.1:5432/lalasearch".to_string()
    });
    PgPool::connect(&database_url)
        .await
        .expect("Failed to connect to PostgreSQL")
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_migrations_apply_cleanly() {
    let pool = test_pool().await;

    let result = sqlx::migrate!("./migrations").run(&pool).await;

    assert!(
        result.is_ok(),
        "Migrations should apply without error: {result:?}"
    );
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_migrations_are_idempotent() {
    let pool = test_pool().await;

    // Run migrations twice — second run should be a no-op
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("First migration run failed");

    let result = sqlx::migrate!("./migrations").run(&pool).await;

    assert!(
        result.is_ok(),
        "Running migrations a second time should succeed: {result:?}"
    );
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_migrations_create_expected_tables() {
    let pool = test_pool().await;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Migrations failed");

    let expected_tables = [
        "tenants",
        "users",
        "sessions",
        "magic_link_tokens",
        "magic_link_send_attempts",
        "org_memberships",
        "org_invitations",
        "crawled_pages",
        "crawl_queue",
        "allowed_domains",
        "robots_cache",
        "crawl_errors",
        "settings",
        "action_history",
    ];

    for table in &expected_tables {
        let row = sqlx::query(
            "SELECT EXISTS (
                SELECT 1 FROM information_schema.tables
                WHERE table_schema = 'public' AND table_name = $1
            ) AS exists",
        )
        .bind(table)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| panic!("Failed to check table {table}: {e}"));

        let exists: bool = row.get("exists");
        assert!(exists, "Table '{table}' should exist after migration");
    }
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_migrations_enable_rls_on_tenant_tables() {
    let pool = test_pool().await;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Migrations failed");

    let rls_tables = [
        "crawled_pages",
        "crawl_queue",
        "allowed_domains",
        "robots_cache",
        "crawl_errors",
        "settings",
        "action_history",
    ];

    for table in &rls_tables {
        let row = sqlx::query(
            "SELECT rowsecurity FROM pg_tables
             WHERE schemaname = 'public' AND tablename = $1",
        )
        .bind(table)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| panic!("Failed to check RLS for {table}: {e}"));

        let rls_enabled: bool = row.get("rowsecurity");
        assert!(rls_enabled, "RLS should be enabled on table '{table}'");
    }
}
