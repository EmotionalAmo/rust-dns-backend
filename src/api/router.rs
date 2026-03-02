use super::handlers;
use super::AppState;
use axum::{
    routing::{delete, get, post, put},
    Router,
};
use std::sync::Arc;
use tower_http::services::{ServeDir, ServeFile};

pub fn routes(state: Arc<AppState>) -> Router {
    let static_dir = state.static_dir.clone();
    Router::new()
        // Health (public)
        .route("/health", get(handlers::health::health_check))
        // Auth (public)
        .route("/api/v1/auth/login", post(handlers::auth::login))
        .route("/api/v1/auth/logout", post(handlers::auth::logout))
        // Change Password (protected)
        .route(
            "/api/v1/auth/change-password",
            post(handlers::auth::change_password),
        )
        // Dashboard (protected)
        .route(
            "/api/v1/dashboard/stats",
            get(handlers::dashboard::get_stats),
        )
        .route(
            "/api/v1/dashboard/query-trend",
            get(handlers::dashboard::get_query_trend),
        )
        .route(
            "/api/v1/dashboard/top-blocked-domains",
            get(handlers::dashboard::get_top_blocked_domains),
        )
        .route(
            "/api/v1/dashboard/top-clients",
            get(handlers::dashboard::get_top_clients),
        )
        // Query log (protected)
        .route("/api/v1/query-log", get(handlers::query_log::list))
        .route("/api/v1/query-log/export", get(handlers::query_log::export))
        // Query log advanced filtering (protected)
        .route(
            "/api/v1/query-log/advanced",
            get(handlers::query_log_advanced::list_advanced),
        )
        .route(
            "/api/v1/query-log/aggregate",
            get(handlers::query_log_advanced::aggregate),
        )
        .route(
            "/api/v1/query-log/top",
            get(handlers::query_log_advanced::top),
        )
        .route(
            "/api/v1/query-log/suggest",
            get(handlers::query_log_advanced::suggest),
        )
        // Query log templates (protected)
        .route(
            "/api/v1/query-log/templates",
            get(handlers::query_log_templates::list),
        )
        .route(
            "/api/v1/query-log/templates",
            post(handlers::query_log_templates::create),
        )
        .route(
            "/api/v1/query-log/templates/{id}",
            get(handlers::query_log_templates::get),
        )
        .route(
            "/api/v1/query-log/templates/{id}",
            put(handlers::query_log_templates::update),
        )
        .route(
            "/api/v1/query-log/templates/{id}",
            delete(handlers::query_log_templates::delete),
        )
        // Filters (protected)
        .route(
            "/api/v1/filters",
            get(handlers::filters::list).post(handlers::filters::create),
        )
        .route(
            "/api/v1/filters/{id}",
            put(handlers::filters::update).delete(handlers::filters::delete),
        )
        .route(
            "/api/v1/filters/{id}/refresh",
            post(handlers::filters::refresh),
        )
        // Rules (protected)
        .route(
            "/api/v1/rules",
            get(handlers::rules::list).post(handlers::rules::create),
        )
        .route("/api/v1/rules/bulk", post(handlers::rules::bulk_action))
        .route(
            "/api/v1/rules/{id}",
            put(handlers::rules::update)
                .post(handlers::rules::toggle)
                .delete(handlers::rules::delete),
        )
        .route(
            "/api/v1/rules/validate",
            post(handlers::rule_validation::validate_rule),
        )
        // DNS Rewrites (protected)
        .route(
            "/api/v1/rewrites",
            get(handlers::rewrites::list).post(handlers::rewrites::create),
        )
        .route(
            "/api/v1/rewrites/{id}",
            put(handlers::rewrites::update).delete(handlers::rewrites::delete),
        )
        // Clients (protected)
        .route(
            "/api/v1/clients",
            get(handlers::clients::list).post(handlers::clients::create),
        )
        .route(
            "/api/v1/clients/{id}",
            put(handlers::clients::update).delete(handlers::clients::delete),
        )
        // Client Groups (protected)
        .route(
            "/api/v1/client-groups",
            get(handlers::client_groups::list_groups).post(handlers::client_groups::create_group),
        )
        .route(
            "/api/v1/client-groups/{id}",
            put(handlers::client_groups::update_group)
                .delete(handlers::client_groups::delete_group),
        )
        .route(
            "/api/v1/client-groups/{id}/members",
            get(handlers::client_groups::get_group_members)
                .post(handlers::client_groups::batch_add_clients)
                .delete(handlers::client_groups::batch_remove_clients),
        )
        .route(
            "/api/v1/clients/batch-move",
            post(handlers::client_groups::batch_move_clients),
        )
        .route(
            "/api/v1/client-groups/{id}/rules",
            get(handlers::client_groups::get_group_rules)
                .post(handlers::client_groups::batch_bind_rules)
                .delete(handlers::client_groups::batch_unbind_rules),
        )
        // Upstreams (protected)
        .route(
            "/api/v1/settings/upstreams",
            get(handlers::upstreams::list).post(handlers::upstreams::create),
        )
        .route(
            "/api/v1/settings/upstreams/health",
            get(handlers::upstreams::get_health),
        )
        .route(
            "/api/v1/settings/upstreams/{id}",
            get(handlers::upstreams::get),
        )
        .route(
            "/api/v1/settings/upstreams/{id}",
            put(handlers::upstreams::update).delete(handlers::upstreams::delete),
        )
        .route(
            "/api/v1/settings/upstreams/{id}/test",
            post(handlers::upstreams::test),
        )
        .route(
            "/api/v1/settings/upstreams/failover",
            post(handlers::upstreams::trigger_failover),
        )
        // Settings (protected)
        .route(
            "/api/v1/settings/dns",
            get(handlers::settings::get_dns).put(handlers::settings::update_dns),
        )
        // Users (admin only)
        .route(
            "/api/v1/users",
            get(handlers::users::list).post(handlers::users::create),
        )
        .route("/api/v1/users/{id}/role", put(handlers::users::update_role))
        // Audit log (admin only)
        .route("/api/v1/audit-log", get(handlers::audit_log::list))
        .route(
            "/api/v1/settings/upstreams/failover-log",
            get(handlers::upstreams::failover_log),
        )
        // Alerts (protected/admin)
        .route(
            "/api/v1/alerts",
            get(handlers::alerts::list_alerts).delete(handlers::alerts::clear_alerts),
        )
        .route(
            "/api/v1/alerts/read-all",
            put(handlers::alerts::mark_all_alerts_read),
        )
        .route(
            "/api/v1/alerts/{id}/read",
            put(handlers::alerts::mark_alert_read),
        )
        // Prometheus metrics (admin only - security fix)
        .route("/metrics", get(handlers::metrics::prometheus_metrics))
        // Backup (admin only)
        .route("/api/v1/admin/backup", get(handlers::backup::create_backup))
        // WebSocket: issue one-time ticket (authenticated), then connect via ticket
        .route("/api/v1/ws/ticket", post(handlers::ws::issue_ticket))
        .route("/api/v1/ws/query-log", get(handlers::ws::query_log_ws))
        // DNS-over-HTTPS (RFC 8484) — public endpoint, no auth required
        .route(
            "/dns-query",
            get(handlers::doh::get_query).post(handlers::doh::post_query),
        )
        // Cache management (admin only)
        .route(
            "/api/v1/settings/cache",
            get(handlers::cache::get_cache_stats).delete(handlers::cache::flush_cache),
        )
        // Filter stats (protected)
        .route(
            "/api/v1/stats/filter",
            get(handlers::filter_stats::get_filter_stats),
        )
        // Domain check (protected)
        .route(
            "/api/v1/dns/check",
            post(handlers::domain_check::check_domains),
        )
        // Insights (protected)
        .route(
            "/api/v1/insights/apps/top",
            get(handlers::insights::top_apps),
        )
        .route(
            "/api/v1/insights/apps/trend",
            get(handlers::insights::app_trend),
        )
        .route(
            "/api/v1/insights/catalog",
            get(handlers::insights::list_catalog),
        )
        .route(
            "/api/v1/insights/domains/top",
            get(handlers::insights::top_domains),
        )
        // Tools (protected)
        .route(
            "/api/v1/tools/sandbox",
            post(handlers::sandbox::test_rule),
        )
        .with_state(state)
        // 前端静态文件 + SPA fallback（必须在 with_state 之后）
        .fallback_service({
            let fallback = format!("{}/index.html", static_dir);
            ServeDir::new(static_dir).fallback(ServeFile::new(fallback))
        })
}
