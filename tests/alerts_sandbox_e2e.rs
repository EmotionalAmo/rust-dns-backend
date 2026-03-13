use axum::http::StatusCode;
use dashmap::DashMap;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::broadcast;

use rust_dns::api::router::routes;
use rust_dns::api::AppState;
use rust_dns::dns::filter::FilterEngine;
use rust_dns::metrics::DnsMetrics;

async fn setup_db() -> PgPool {
    let database_url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://postgres:postgres@localhost:5432/rust_dns_test".to_string()
    });

    let pool = PgPool::connect(&database_url).await.unwrap();
    sqlx::migrate!("./src/db/migrations")
        .run(&pool)
        .await
        .unwrap();
    pool
}

async fn build_test_state() -> Arc<AppState> {
    let db = setup_db().await;
    let filter = Arc::new(FilterEngine::new(db.clone()).await.unwrap());
    let metrics = Arc::new(DnsMetrics::default());
    let (query_log_tx, _) = broadcast::channel(16);

    let test_cfg = rust_dns::config::Config {
        dns: rust_dns::config::DnsConfig {
            port: 15402,
            bind: "127.0.0.1".to_string(),
            upstreams: vec!["https://1.1.1.1/dns-query".to_string()],
            prefer_ipv4: true,
            doh_enabled: false,
            dot_enabled: false,
            rewrite_ttl: 300,
        },
        api: rust_dns::config::ApiConfig {
            port: 18102,
            bind: "127.0.0.1".to_string(),
            cors_allowed_origins: vec![],
            static_dir: "frontend/dist".to_string(),
        },
        database: rust_dns::config::DatabaseConfig {
            url: "postgres://postgres:postgres@localhost:5432/rust_dns".to_string(),
            query_log_retention_days: 7,
        },
        auth: rust_dns::config::AuthConfig {
            jwt_secret: "test_secret_32_bytes_long_string_123".to_string(),
            jwt_expiry_hours: 1,
            allow_default_password: true,
        },
        logging: Default::default(),
    };

    let dns_handler = rust_dns::dns::build_handler(
        &test_cfg,
        db.clone(),
        filter.clone(),
        metrics.clone(),
        query_log_tx.clone(),
        std::sync::Arc::new(rust_dns::db::app_catalog_cache::AppCatalogCache::new()),
    )
    .await
    .unwrap();

    Arc::new(AppState {
        db,
        db_url: test_cfg.database.url.clone(),
        filter,
        jwt_secret: test_cfg.auth.jwt_secret,
        jwt_expiry_hours: 1,
        metrics,
        query_log_tx,
        ws_tickets: DashMap::new(),
        login_attempts: DashMap::new(),
        dns_handler,
        rule_validation_cache: Arc::new(moka::future::Cache::builder().max_capacity(100).build()),
        client_config_cache: None,
        static_dir: "frontend/dist".to_string(),
        allow_default_password: test_cfg.auth.allow_default_password,
        upstream_health: DashMap::new(),
        suggest_cache: Arc::new(moka::future::Cache::builder().max_capacity(100).build()),
        token_blacklist: DashMap::new(),
    })
}

#[tokio::test]
async fn test_sandbox_api() {
    let state = build_test_state().await;
    let app = routes(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();

    // Test Sandbox Rules with NO auth required since Sandbox is protected... Wait, we need a token.
    // Let's generate a token using the jwt utils.
    let token = rust_dns::auth::jwt::generate(
        "test-user-id",
        "admin",
        "super_admin",
        "test_secret_32_bytes_long_string_123",
        1,
    )
    .unwrap();

    let res = client
        .post(format!("http://{}/api/v1/tools/sandbox", addr))
        .bearer_auth(&token)
        .json(&json!({
            "rule": "||example.com^",
            "test_domains": ["example.com", "test.example.com", "other.com"]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body: Value = res.json().await.unwrap();

    assert!(body["rule_valid"].as_bool().unwrap());
    assert_eq!(body["rule_type"].as_str().unwrap(), "block");
    let results = body["results"].as_array().unwrap();
    assert_eq!(results[0]["domain"], "example.com");
    assert_eq!(results[0]["status"], "blocked");
    assert_eq!(results[2]["domain"], "other.com");
    assert_eq!(results[2]["status"], "unmatched");
}

#[tokio::test]
async fn test_alerts_api() {
    let state = build_test_state().await;
    let app = routes(state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Clean up all stale test alerts for isolation
    let _ = sqlx::query("DELETE FROM alerts").execute(&state.db).await;

    // Insert alert into DB
    let now_str = chrono::Utc::now().to_rfc3339();
    sqlx::query("INSERT INTO alerts (id, alert_type, message, is_read, created_at) VALUES ($1, $2, $3, $4, $5)")
        .bind("123")
        .bind("system")
        .bind("Test Alert")
        .bind(0i32)
        .bind(&now_str)
        .execute(&state.db)
        .await
        .unwrap();

    let client = reqwest::Client::new();
    let token = rust_dns::auth::jwt::generate(
        "user1",
        "admin",
        "super_admin",
        "test_secret_32_bytes_long_string_123",
        1,
    )
    .unwrap();

    // GET /api/v1/alerts
    let res = client
        .get(format!("http://{}/api/v1/alerts", addr))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["total"].as_i64().unwrap(), 1);

    // PUT /api/v1/alerts/123/read
    let res = client
        .put(format!("http://{}/api/v1/alerts/123/read", addr))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // GET again and filter by is_read=true
    let res = client
        .get(format!("http://{}/api/v1/alerts?is_read=true", addr))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();

    let body: Value = res.json().await.unwrap();
    assert_eq!(body["total"].as_i64().unwrap(), 1);
}
