//! HTTP 层 RBAC 集成测试
//!
//! 测试三种场景：
//!   1. 403 — read_only 角色访问 admin-only 端点
//!   2. 401 — 无 token 访问 admin-only 端点
//!   3. 200 — super_admin 角色访问 GET 端点（断言 != 403）
//!
//! 所有测试使用 oneshot 模式，不绑定 TCP 端口，不依赖 ConnectInfo。

use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use dashmap::DashMap;
use moka::future::Cache as MokaCache;
use std::sync::Arc;
use tokio::sync::broadcast;
use tower::ServiceExt;

use rust_dns::api::validators::rule::RuleValidationResponse;
use rust_dns::api::{build_app, AppState};
use rust_dns::auth::jwt;
use rust_dns::dns::filter::FilterEngine;
use rust_dns::metrics::DnsMetrics;

// ── 常量 ───────────────────────────────────────────────────────────────────

const TEST_SECRET: &str = "test-jwt-secret-for-integration-tests-only-32chars";
const TEST_USER_ID: &str = "00000000-0000-0000-0000-000000000001";

// ── Helper ─────────────────────────────────────────────────────────────────

/// 构建测试专用 PostgreSQL 数据库
async fn setup_db() -> sqlx::PgPool {
    let database_url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://postgres:postgres@localhost:5432/rust_dns_test".to_string()
    });

    let pool = sqlx::PgPool::connect(&database_url)
        .await
        .expect("Failed to connect to PostgreSQL");

    sqlx::migrate!("./src/db/migrations")
        .run(&pool)
        .await
        .expect("Migration failed");

    pool
}

/// 构建测试 App（不启动 TCP 监听器）
async fn build_rbac_test_app() -> axum::Router {
    let db = setup_db().await;
    let filter = Arc::new(
        FilterEngine::new(db.clone())
            .await
            .expect("Failed to build FilterEngine"),
    );
    let metrics = Arc::new(DnsMetrics::default());
    let (query_log_tx, _) = broadcast::channel::<serde_json::Value>(16);

    let test_cfg = rust_dns::config::Config {
        dns: rust_dns::config::DnsConfig {
            port: 15400,
            bind: "127.0.0.1".to_string(),
            upstreams: vec!["https://1.1.1.1/dns-query".to_string()],
            prefer_ipv4: true,
            doh_enabled: false,
            dot_enabled: false,
            rewrite_ttl: 300,
        },
        api: rust_dns::config::ApiConfig {
            port: 18100,
            bind: "127.0.0.1".to_string(),
            cors_allowed_origins: vec!["http://localhost:5173".to_string()],
            static_dir: "frontend/dist".to_string(),
        },
        database: rust_dns::config::DatabaseConfig {
            url: "postgres://postgres:postgres@localhost:5432/rust_dns".to_string(),
            query_log_retention_days: 7,
        },
        auth: rust_dns::config::AuthConfig {
            jwt_secret: TEST_SECRET.to_string(),
            jwt_expiry_hours: 1,
            allow_default_password: false,
        },
        logging: Default::default(),
    };

    let dns_handler = rust_dns::dns::build_handler(
        &test_cfg,
        db.clone(),
        filter.clone(),
        metrics.clone(),
        query_log_tx.clone(),
        Arc::new(rust_dns::db::app_catalog_cache::AppCatalogCache::new()),
    )
    .await
    .expect("Failed to build DnsHandler");

    let state = Arc::new(AppState {
        db,
        db_url: test_cfg.database.url.clone(),
        filter,
        jwt_secret: TEST_SECRET.to_string(),
        jwt_expiry_hours: 1,
        metrics,
        query_log_tx,
        ws_tickets: DashMap::new(),
        login_attempts: DashMap::new(),
        dns_handler,
        rule_validation_cache: Arc::new(
            MokaCache::<String, RuleValidationResponse>::builder()
                .max_capacity(1000)
                .time_to_live(std::time::Duration::from_secs(300))
                .build(),
        ),
        client_config_cache: None,
        static_dir: "frontend/dist".to_string(),
        allow_default_password: test_cfg.auth.allow_default_password,
        upstream_health: DashMap::new(),
        suggest_cache: Arc::new(
            MokaCache::<String, Vec<String>>::builder()
                .max_capacity(100)
                .build(),
        ),
        token_blacklist: DashMap::new(),
    });

    let cors = tower_http::cors::CorsLayer::new();
    build_app(state, cors)
}

/// 生成指定角色的 Bearer token 字符串
fn make_token(role: &str) -> String {
    let token = jwt::generate(TEST_USER_ID, "testuser", role, TEST_SECRET, 1)
        .expect("Should generate JWT token");
    format!("Bearer {token}")
}

// ── 403 测试：read_only 角色访问 admin-only 端点 ───────────────────────────

#[tokio::test]
async fn test_403_get_users_read_only() {
    let app = build_rbac_test_app().await;
    let token = make_token("read_only");

    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/users")
        .header(header::AUTHORIZATION, token)
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_403_post_users_read_only() {
    let app = build_rbac_test_app().await;
    let token = make_token("read_only");

    // 403 在 body 解析前由 AdminUser extractor 返回，body 内容无关紧要
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/users")
        .header(header::AUTHORIZATION, token)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_403_get_audit_log_read_only() {
    let app = build_rbac_test_app().await;
    let token = make_token("read_only");

    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/audit-log")
        .header(header::AUTHORIZATION, token)
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_403_get_metrics_read_only() {
    let app = build_rbac_test_app().await;
    let token = make_token("read_only");

    let req = Request::builder()
        .method("GET")
        .uri("/metrics")
        .header(header::AUTHORIZATION, token)
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_403_get_backup_read_only() {
    let app = build_rbac_test_app().await;
    let token = make_token("read_only");

    // AdminUser extractor 在 handler 逻辑前拒绝，pg_dump 不会被调用
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/admin/backup")
        .header(header::AUTHORIZATION, token)
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_403_get_settings_cache_read_only() {
    let app = build_rbac_test_app().await;
    let token = make_token("read_only");

    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/settings/cache")
        .header(header::AUTHORIZATION, token)
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_403_delete_settings_cache_read_only() {
    let app = build_rbac_test_app().await;
    let token = make_token("read_only");

    let req = Request::builder()
        .method("DELETE")
        .uri("/api/v1/settings/cache")
        .header(header::AUTHORIZATION, token)
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// ── 401 测试：无 token 访问 admin-only 端点 ────────────────────────────────

#[tokio::test]
async fn test_401_get_users_no_token() {
    let app = build_rbac_test_app().await;

    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/users")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_401_post_users_no_token() {
    let app = build_rbac_test_app().await;

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/users")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_401_get_audit_log_no_token() {
    let app = build_rbac_test_app().await;

    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/audit-log")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_401_get_metrics_no_token() {
    let app = build_rbac_test_app().await;

    let req = Request::builder()
        .method("GET")
        .uri("/metrics")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_401_get_backup_no_token() {
    let app = build_rbac_test_app().await;

    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/admin/backup")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_401_get_settings_cache_no_token() {
    let app = build_rbac_test_app().await;

    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/settings/cache")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_401_delete_settings_cache_no_token() {
    let app = build_rbac_test_app().await;

    let req = Request::builder()
        .method("DELETE")
        .uri("/api/v1/settings/cache")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── 200 测试：super_admin 角色访问 GET 端点（断言 != 403） ─────────────────

#[tokio::test]
async fn test_super_admin_get_users_not_forbidden() {
    let app = build_rbac_test_app().await;
    let token = make_token("super_admin");

    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/users")
        .header(header::AUTHORIZATION, token)
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_ne!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "super_admin should not be denied access to GET /api/v1/users"
    );
}

#[tokio::test]
async fn test_super_admin_get_audit_log_not_forbidden() {
    let app = build_rbac_test_app().await;
    let token = make_token("super_admin");

    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/audit-log")
        .header(header::AUTHORIZATION, token)
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_ne!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "super_admin should not be denied access to GET /api/v1/audit-log"
    );
}

#[tokio::test]
async fn test_super_admin_get_metrics_not_forbidden() {
    let app = build_rbac_test_app().await;
    let token = make_token("super_admin");

    let req = Request::builder()
        .method("GET")
        .uri("/metrics")
        .header(header::AUTHORIZATION, token)
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_ne!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "super_admin should not be denied access to GET /metrics"
    );
}

#[tokio::test]
async fn test_super_admin_get_settings_cache_not_forbidden() {
    let app = build_rbac_test_app().await;
    let token = make_token("super_admin");

    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/settings/cache")
        .header(header::AUTHORIZATION, token)
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_ne!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "super_admin should not be denied access to GET /api/v1/settings/cache"
    );
}
