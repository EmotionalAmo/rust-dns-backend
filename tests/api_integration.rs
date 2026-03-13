//! API 集成测试
//!
//! 两种测试模式：
//! 1. oneshot 模式：直接调用 router，不绑定端口（适合不需要 ConnectInfo 的端点）
//! 2. bound server 模式：绑定到随机端口，通过真实 HTTP 请求测试（适合 login 等需要 ConnectInfo 的端点）
//!
//! 覆盖端点：
//!   - GET  /health
//!   - POST /api/v1/auth/login  (成功 / 错误密码)
//!   - POST /api/v1/auth/logout
//!   - GET  /api/v1/rules       (需要 Bearer token)
//!   - POST /api/v1/rules       (创建规则)
//!   - DELETE /api/v1/rules/{id}
//!   - GET  /api/v1/query-log   (需要 Bearer token / 参数过滤)

use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use dashmap::DashMap;
use http_body_util::BodyExt;
use moka::future::Cache as MokaCache;
use serde_json::Value;
use sqlx::PgPool;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::broadcast;
use tower::ServiceExt; // for .oneshot() // for .collect()

// ── 内部 crate 引用 ────────────────────────────────────────────────────────────
// 集成测试与被测试 crate 在同一 workspace，直接引用
use rust_dns::api::validators::rule::RuleValidationResponse;
use rust_dns::api::{build_app, AppState};
use rust_dns::dns::filter::FilterEngine;
use rust_dns::metrics::DnsMetrics;

/// 构建测试专用 PostgreSQL 数据库，运行所有 migration，并插入 admin 用户
async fn setup_db() -> PgPool {
    let database_url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://postgres:postgres@localhost:5432/rust_dns_test".to_string()
    });

    let pool = PgPool::connect(&database_url)
        .await
        .expect("Failed to connect to PostgreSQL");

    sqlx::migrate!("./src/db/migrations")
        .run(&pool)
        .await
        .expect("Migration failed");

    // Clean up any existing test data to ensure fresh state
    let _ = sqlx::query("DELETE FROM users WHERE username = 'admin'")
        .execute(&pool)
        .await
        .ok(); // ignore if no rows deleted

    // Insert test admin user (password: admin) or update password if already exists
    let password_hash = rust_dns::auth::password::hash("admin").expect("Failed to hash password");
    let id = uuid::Uuid::new_v4().to_string();

    let now_str = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO users (id, username, password, role, is_active, created_at, updated_at)
         VALUES ($1, 'admin', $2, 'super_admin', 1, $3, $3)
         ON CONFLICT (username) DO UPDATE SET password = EXCLUDED.password",
    )
    .bind(&id)
    .bind(&password_hash)
    .bind(&now_str)
    .execute(&pool)
    .await
    .expect("Failed to seed admin user");

    pool
}

/// 启动真实 TCP 监听的测试服务器，返回绑定地址和 AppState。
/// 用于需要 ConnectInfo（如 login）的测试。
/// 返回的 task handle 在 Drop 时自动终止服务器。
async fn start_test_server() -> (String, Arc<AppState>) {
    let (_, state) = build_test_app().await;

    let cors = tower_http::cors::CorsLayer::new();
    let app = rust_dns::api::build_app(state.clone(), cors);

    // 绑定到随机端口
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind test server");
    let addr = listener.local_addr().expect("Failed to get local addr");

    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .ok();
    });

    let base_url = format!("http://127.0.0.1:{}", addr.port());
    (base_url, state)
}

/// 构建完整测试 App（不启动 TCP 监听器）。
/// DnsHandler 在集成测试中设为 None-safe 模式：
/// 只测试不涉及 DNS 解析的端点（auth、rules、query-log）。
async fn build_test_app() -> (axum::Router, Arc<AppState>) {
    let db = setup_db().await;
    let filter = Arc::new(
        FilterEngine::new(db.clone())
            .await
            .expect("Failed to build FilterEngine"),
    );
    let metrics = Arc::new(DnsMetrics::default());
    let (query_log_tx, _) = broadcast::channel::<serde_json::Value>(16);

    // 集成测试中不启动真实 DNS handler，
    // 但 AppState 需要 dns_handler: Arc<DnsHandler>。
    // 用一个占位 Arc 绕过：构建最小化 DnsHandler。
    let test_cfg = rust_dns::config::Config {
        dns: rust_dns::config::DnsConfig {
            port: 15399, // 随机高端口，不实际监听
            bind: "127.0.0.1".to_string(),
            upstreams: vec!["https://1.1.1.1/dns-query".to_string()],
            prefer_ipv4: true,
            doh_enabled: false,
            dot_enabled: false,
            rewrite_ttl: 300,
        },
        api: rust_dns::config::ApiConfig {
            port: 18099,
            bind: "127.0.0.1".to_string(),
            cors_allowed_origins: vec!["http://localhost:5173".to_string()],
            static_dir: "frontend/dist".to_string(),
        },
        database: rust_dns::config::DatabaseConfig {
            url: "postgres://postgres:postgres@localhost:5432/rust_dns".to_string(),
            query_log_retention_days: 7,
        },
        auth: rust_dns::config::AuthConfig {
            jwt_secret: "test-jwt-secret-for-integration-tests-only-32chars".to_string(),
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
        std::sync::Arc::new(rust_dns::db::app_catalog_cache::AppCatalogCache::new()),
    )
    .await
    .expect("Failed to build DnsHandler");

    let jwt_secret = test_cfg.auth.jwt_secret.clone();
    let state = Arc::new(AppState {
        db,
        filter,
        jwt_secret,
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

    // CORS 层用空配置（测试中不需要）
    let cors = tower_http::cors::CorsLayer::new();
    let app = build_app(state.clone(), cors);
    (app, state)
}

/// 辅助：从响应 Body 读取 JSON Value
async fn body_json(body: axum::body::Body) -> Value {
    let bytes = body
        .collect()
        .await
        .expect("Failed to collect body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("Body is not valid JSON")
}

// ═══════════════════════════════════════════════════════════════════════════════
// Health Check
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_health_check() {
    let (app, _) = build_test_app().await;

    let req = Request::builder()
        .method("GET")
        .uri("/health")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Auth: Login
// login handler 使用 ConnectInfo<SocketAddr>，需要绑定到真实端口的 HTTP 请求
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_login_success_returns_token() {
    let (base_url, _) = start_test_server().await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/v1/auth/login", base_url))
        .json(&serde_json::json!({"username": "admin", "password": "admin"}))
        .send()
        .await
        .expect("Request failed");

    assert_eq!(resp.status().as_u16(), 200);
    let json: Value = resp.json().await.expect("Failed to parse JSON");
    assert!(json["token"].is_string(), "Response should contain a token");
    assert_eq!(json["role"], "super_admin");
    assert!(json["expires_in"].is_number());
}

#[tokio::test]
async fn test_login_wrong_password_returns_401() {
    let (base_url, _) = start_test_server().await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/v1/auth/login", base_url))
        .json(&serde_json::json!({"username": "admin", "password": "wrongpassword"}))
        .send()
        .await
        .expect("Request failed");

    assert_eq!(resp.status().as_u16(), 401);
}

#[tokio::test]
async fn test_login_unknown_user_returns_401() {
    let (base_url, _) = start_test_server().await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/v1/auth/login", base_url))
        .json(&serde_json::json!({"username": "nonexistent", "password": "anything"}))
        .send()
        .await
        .expect("Request failed");

    assert_eq!(resp.status().as_u16(), 401);
}

#[tokio::test]
async fn test_login_rate_limit_after_5_failures() {
    let (base_url, state) = start_test_server().await;

    // 预先将 127.0.0.1 的失败次数设为 5（MAX_LOGIN_FAILURES）
    let ip = "127.0.0.1";
    state
        .login_attempts
        .insert(ip.to_string(), (5u32, Instant::now()));

    // 第 6 次登录应触发限速（即使密码正确）
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/v1/auth/login", base_url))
        .json(&serde_json::json!({"username": "admin", "password": "admin"}))
        .send()
        .await
        .expect("Request failed");

    assert_eq!(
        resp.status().as_u16(),
        429,
        "Should be rate limited after 5 failures"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Auth: Protected routes without token
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_protected_route_without_token_returns_401() {
    let (app, _) = build_test_app().await;

    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/rules")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_protected_route_with_invalid_token_returns_401() {
    let (app, _) = build_test_app().await;

    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/rules")
        .header(header::AUTHORIZATION, "Bearer invalid.jwt.token")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Auth: Logout
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_logout_always_succeeds() {
    let (app, _) = build_test_app().await;

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/logout")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp.into_body()).await;
    assert_eq!(json["success"], true);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Rules CRUD
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_list_rules_requires_auth() {
    let (app, _) = build_test_app().await;

    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/rules")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_list_rules_with_valid_token() {
    let (app, state) = build_test_app().await;

    // 直接生成 token，不走 login HTTP 路径（绕过 ConnectInfo 限制）
    let token =
        rust_dns::auth::jwt::generate("test-user-id", "admin", "super_admin", &state.jwt_secret, 1)
            .expect("Should generate token");

    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/rules")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp.into_body()).await;
    assert!(json["data"].is_array(), "Response should have data array");
    assert!(json["total"].is_number());
}

#[tokio::test]
async fn test_create_rule_and_list() {
    let (app, state) = build_test_app().await;

    let token =
        rust_dns::auth::jwt::generate("test-user-id", "admin", "super_admin", &state.jwt_secret, 1)
            .expect("Should generate token");

    // 创建规则
    let create_req = Request::builder()
        .method("POST")
        .uri("/api/v1/rules")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"rule":"||test-block.example.com^","comment":"integration test rule"}"#,
        ))
        .unwrap();

    let create_resp = app.clone().oneshot(create_req).await.unwrap();
    assert_eq!(
        create_resp.status(),
        StatusCode::OK,
        "Rule creation should succeed"
    );
    let created = body_json(create_resp.into_body()).await;
    assert_eq!(created["rule"], "||test-block.example.com^");
    assert!(created["id"].is_string(), "Created rule should have an ID");

    let rule_id = created["id"].as_str().unwrap().to_string();

    // 列出规则，验证刚创建的规则存在
    let list_req = Request::builder()
        .method("GET")
        .uri("/api/v1/rules")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let list_resp = app.clone().oneshot(list_req).await.unwrap();
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_json = body_json(list_resp.into_body()).await;
    let data = list_json["data"].as_array().unwrap();
    let found = data.iter().any(|r| r["id"] == rule_id);
    assert!(found, "Newly created rule should appear in list");

    // 删除规则
    let delete_req = Request::builder()
        .method("DELETE")
        .uri(format!("/api/v1/rules/{}", rule_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let delete_resp = app.clone().oneshot(delete_req).await.unwrap();
    assert_eq!(delete_resp.status(), StatusCode::OK);
    let delete_json = body_json(delete_resp.into_body()).await;
    assert_eq!(delete_json["success"], true);

    // 再次列出，确认规则已删除
    let list_req2 = Request::builder()
        .method("GET")
        .uri("/api/v1/rules")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let list_resp2 = app.oneshot(list_req2).await.unwrap();
    let list_json2 = body_json(list_resp2.into_body()).await;
    let data2 = list_json2["data"].as_array().unwrap();
    let still_found = data2.iter().any(|r| r["id"] == rule_id);
    assert!(!still_found, "Deleted rule should not appear in list");
}

#[tokio::test]
async fn test_create_rule_empty_body_returns_400() {
    let (app, state) = build_test_app().await;

    let token =
        rust_dns::auth::jwt::generate("test-user-id", "admin", "super_admin", &state.jwt_secret, 1)
            .expect("Should generate token");

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/rules")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"rule":""}"#))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "Empty rule should be rejected"
    );
}

#[tokio::test]
async fn test_delete_nonexistent_rule_returns_404() {
    let (app, state) = build_test_app().await;

    let token =
        rust_dns::auth::jwt::generate("test-user-id", "admin", "super_admin", &state.jwt_secret, 1)
            .expect("Should generate token");

    let req = Request::builder()
        .method("DELETE")
        .uri("/api/v1/rules/nonexistent-uuid-12345")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Query Log
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_query_log_requires_auth() {
    let (app, _) = build_test_app().await;

    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/query-log")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_query_log_empty_result() {
    let (app, state) = build_test_app().await;

    // Ensure clean state for this test
    let _ = sqlx::query("DELETE FROM query_log")
        .execute(&state.db)
        .await;

    let token =
        rust_dns::auth::jwt::generate("test-user-id", "admin", "super_admin", &state.jwt_secret, 1)
            .expect("Should generate token");

    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/query-log")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp.into_body()).await;
    assert!(json["data"].is_array());
    assert_eq!(json["total"], 0);
    assert_eq!(json["returned"], 0);
    assert_eq!(json["offset"], 0);
    assert_eq!(json["limit"], 100);
}

#[tokio::test]
async fn test_query_log_with_status_filter() {
    let (app, state) = build_test_app().await;

    // Ensure clean state for this test
    let _ = sqlx::query("DELETE FROM query_log")
        .execute(&state.db)
        .await;

    // 插入测试日志数据
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO query_log (time, client_ip, question, qtype, status, elapsed_ns)
         VALUES ($1, '192.168.1.1', 'blocked.example.com', 'A', 'blocked', 1000000)",
    )
    .bind(&now)
    .execute(&state.db)
    .await
    .expect("Failed to insert test log");

    sqlx::query(
        "INSERT INTO query_log (time, client_ip, question, qtype, status, elapsed_ns)
         VALUES ($1, '192.168.1.1', 'allowed.example.com', 'A', 'allowed', 5000000)",
    )
    .bind(&now)
    .execute(&state.db)
    .await
    .expect("Failed to insert test log");

    let token =
        rust_dns::auth::jwt::generate("test-user-id", "admin", "super_admin", &state.jwt_secret, 1)
            .expect("Should generate token");

    // 过滤 status=blocked
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/query-log?status=blocked")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp.into_body()).await;
    let data = json["data"].as_array().unwrap();
    assert_eq!(json["total"], 1, "Should find exactly 1 blocked entry");
    assert!(
        data.iter().all(|r| r["status"] == "blocked"),
        "All results should be blocked"
    );
}

#[tokio::test]
async fn test_query_log_pagination_limit() {
    let (app, state) = build_test_app().await;

    // Ensure clean state for this test
    let _ = sqlx::query("DELETE FROM query_log")
        .execute(&state.db)
        .await;

    // 插入 5 条日志
    let now = chrono::Utc::now().to_rfc3339();
    for i in 0..5 {
        sqlx::query(
            "INSERT INTO query_log (time, client_ip, question, qtype, status, elapsed_ns)
             VALUES ($1, '10.0.0.1', $2, 'A', 'allowed', 1000000)",
        )
        .bind(&now)
        .bind(format!("domain{}.com", i))
        .execute(&state.db)
        .await
        .expect("Failed to insert");
    }

    let token =
        rust_dns::auth::jwt::generate("test-user-id", "admin", "super_admin", &state.jwt_secret, 1)
            .expect("Should generate token");

    // 请求 limit=2
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/query-log?limit=2")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp.into_body()).await;
    assert_eq!(json["returned"], 2, "Should return exactly 2 entries");
    assert_eq!(json["total"], 5, "Total should be 5");
    assert_eq!(json["limit"], 2);

    let first = json["data"][0].as_object().expect("Entry should be object");
    assert!(first.contains_key("id"));
    assert!(first.contains_key("time"));
    assert!(first.contains_key("client_ip"));
    assert!(first.contains_key("client_name"));
    assert!(first.contains_key("question"));
    assert!(first.contains_key("qtype"));
    assert!(first.contains_key("answer"));
    assert!(first.contains_key("status"));
    assert!(first.contains_key("reason"));
    assert!(first.contains_key("elapsed_ns"));
    assert!(first.contains_key("upstream_ns"));
}

#[tokio::test]
async fn test_query_log_invalid_status_returns_400() {
    let (app, state) = build_test_app().await;

    let token =
        rust_dns::auth::jwt::generate("test-user-id", "admin", "super_admin", &state.jwt_secret, 1)
            .expect("Should generate token");

    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/query-log?status=invalid")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ═══════════════════════════════════════════════════════════════════════════════
// DNS 重写规则 CRUD 集成测试
// ═══════════════════════════════════════════════════════════════════════════════

/// 辅助：获取管理员 JWT token（使用 auth::jwt 直接生成，不需要真实 HTTP login）
fn admin_token(state: &AppState) -> String {
    rust_dns::auth::jwt::generate("admin-id", "admin", "super_admin", &state.jwt_secret, 1)
        .expect("Should generate token")
}

#[tokio::test]
async fn test_rewrite_create_and_list() {
    let (app, state) = build_test_app().await;

    // Clean up stale test data
    let _ = sqlx::query("DELETE FROM dns_rewrites WHERE domain = 'test.local'")
        .execute(&state.db)
        .await;

    let token = admin_token(&state);

    // 1. 创建 DNS 重写规则
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/rewrites")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"domain":"test.local","answer":"127.0.0.1"}"#,
        ))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "Create should return 200");
    let json = body_json(resp.into_body()).await;
    assert_eq!(json["domain"], "test.local");
    assert_eq!(json["answer"], "127.0.0.1");
    assert!(json["id"].is_string(), "Should have an id");

    // 2. 列出重写规则，验证刚创建的存在（响应格式：{"data":[...],"total":N}）
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/rewrites")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp.into_body()).await;
    let list = json["data"].as_array().expect("Should have 'data' array");
    assert!(!list.is_empty(), "Should have at least one rewrite");
    assert!(
        list.iter().any(|r| r["domain"] == "test.local"),
        "Should contain test.local"
    );
}

#[tokio::test]
async fn test_rewrite_update_and_delete() {
    let (app, state) = build_test_app().await;

    // Clean up stale test data
    let _ = sqlx::query("DELETE FROM dns_rewrites WHERE domain IN ('update-test.local')")
        .execute(&state.db)
        .await;

    let token = admin_token(&state);

    // 1. 创建
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/rewrites")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"domain":"update-test.local","answer":"10.0.0.1"}"#,
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "Create should return 200");
    let created = body_json(resp.into_body()).await;
    let id = created["id"].as_str().expect("Should have id").to_string();

    // 2. 更新 answer
    let update_body = r#"{"domain":"update-test.local","answer":"192.168.1.1"}"#.to_string();
    let req = Request::builder()
        .method("PUT")
        .uri(format!("/api/v1/rewrites/{}", id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(update_body))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "Update should return 200");
    let updated = body_json(resp.into_body()).await;
    assert_eq!(updated["answer"], "192.168.1.1", "Answer should be updated");

    // 3. 删除（返回 200 + {"success": true}）
    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/api/v1/rewrites/{}", id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "Delete should return 200");

    // 4. 验证已删除（响应格式：{"data":[...],"total":N}）
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/rewrites")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let list = body_json(resp.into_body()).await;
    let list = list["data"].as_array().expect("Should have 'data' array");
    assert!(
        !list.iter().any(|r| r["domain"] == "update-test.local"),
        "Deleted entry should not appear in list"
    );
}

#[tokio::test]
async fn test_rewrite_duplicate_domain_rejected() {
    let (app, state) = build_test_app().await;

    // Clean up stale test data
    let _ = sqlx::query("DELETE FROM dns_rewrites WHERE domain = 'dup.local'")
        .execute(&state.db)
        .await;

    let token = admin_token(&state);

    // 创建第一条
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/rewrites")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"domain":"dup.local","answer":"1.2.3.4"}"#))
        .unwrap();
    app.clone().oneshot(req).await.unwrap();

    // 尝试创建重复域名
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/rewrites")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"domain":"dup.local","answer":"5.6.7.8"}"#))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert!(
        resp.status() == StatusCode::CONFLICT || resp.status().is_client_error(),
        "Duplicate domain should be rejected (got {})",
        resp.status()
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 客户端 CRUD 集成测试
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_client_create_and_list() {
    let (app, state) = build_test_app().await;
    let token = admin_token(&state);

    // 1. 创建客户端
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/clients")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"name":"test-device","identifiers":["192.168.1.200"],"filter_enabled":true}"#,
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "Create should return 200");
    let created = body_json(resp.into_body()).await;
    assert_eq!(created["name"], "test-device");
    assert!(created["id"].is_string());

    // 2. 列出，验证存在（响应格式：{"data":[...],"total":N}）
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/clients")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let list = body_json(resp.into_body()).await;
    let list = list["data"].as_array().expect("Should have 'data' array");
    assert!(
        list.iter().any(|c| c["name"] == "test-device"),
        "Should find test-device in list"
    );
}

#[tokio::test]
async fn test_client_update_filter_enabled() {
    let (app, state) = build_test_app().await;
    let token = admin_token(&state);

    // 1. 创建客户端（filter_enabled = true）
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/clients")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"name":"filter-test","identifiers":["10.0.0.50"],"filter_enabled":true}"#,
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "Create should return 200");
    let created = body_json(resp.into_body()).await;
    let id = created["id"].as_str().unwrap().to_string();

    // 2. 更新 filter_enabled = false
    let req = Request::builder()
        .method("PUT")
        .uri(format!("/api/v1/clients/{}", id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"name":"filter-test","identifiers":["10.0.0.50"],"filter_enabled":false}"#,
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "Update should return 200");
    let updated = body_json(resp.into_body()).await;
    assert_eq!(
        updated["filter_enabled"], false,
        "filter_enabled should be updated to false"
    );
}
