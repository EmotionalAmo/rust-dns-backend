// API Routes for Query Log Advanced Filtering
// File: src/api/routes/query_log.rs
// Author: ui-duarte
// Date: 2026-02-20

use axum::{
    routing::{get, post, put, delete},
    Router,
};
use std::sync::Arc;

use crate::api::AppState;
use crate::api::handlers::query_log::{list, export};
use crate::api::handlers::query_log_advanced::{list_advanced, aggregate, top, suggest};
use crate::api::handlers::query_log_templates::{list as list_templates, create as create_template, get as get_template, update as update_template, delete as delete_template};
use crate::api::middleware::auth::AuthUser;
use crate::api::middleware::rbac::AdminUser;

pub fn create_routes() -> Router<Arc<AppState>> {
    Router::new()
        // ===== 基础查询（保持兼容） =====
        .route("/query-log", get(list))
        .route("/query-log/export", get(export))

        // ===== 高级过滤 =====
        .route("/query-log/advanced", get(list_advanced))
        .route("/query-log/aggregate", get(aggregate))
        .route("/query-log/top", get(top))
        .route("/query-log/suggest", get(suggest))

        // ===== 查询模板 CRUD =====
        .route("/query-log/templates", get(list_templates))
        .route("/query-log/templates", post(create_template))
        .route("/query-log/templates/:id", get(get_template))
        .route("/query-log/templates/:id", put(update_template))
        .route("/query-log/templates/:id", delete(delete_template))
}

// 注册到主路由
// 在 src/api/mod.rs 中：
// pub mod routes;
// use routes::query_log::create_routes;
//
// pub fn create_app(state: Arc<AppState>) -> Router {
//     Router::new()
//         .nest("/api/v1", create_routes())
//         .with_state(state)
// }
