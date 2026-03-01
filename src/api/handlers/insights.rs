use crate::api::middleware::auth::AuthUser;
use crate::api::AppState;
use crate::error::AppResult;
use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use std::sync::Arc;

#[derive(Deserialize)]
pub struct TopAppsParams {
    pub hours: Option<i64>,
    pub limit: Option<i64>,
    pub category: Option<String>,
    pub status: Option<String>,
}

pub async fn top_apps(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(params): Query<TopAppsParams>,
) -> AppResult<Json<Value>> {
    let hours = params.hours.unwrap_or(24).clamp(1, 720);
    let limit = params.limit.unwrap_or(20).clamp(1, 100);
    let category = params.category.unwrap_or_default();
    let status = params.status.unwrap_or_default();

    // 直接用预计算的 app_id 列做 JOIN，无需 LIKE '%.' || domain 全表扫描。
    // Migration 015 在写入时同步计算 app_id，查询复杂度从 O(ql × ad) 降至 O(ql)。
    let rows = sqlx::query(
        "SELECT ac.id, ac.app_name, ac.category, ac.icon, \
         COUNT(*) AS total_queries, \
         COUNT(DISTINCT ql.client_ip) AS unique_clients, \
         SUM(CASE WHEN ql.status = 'blocked' THEN 1 ELSE 0 END) AS blocked_queries, \
         MAX(ql.time) AS last_seen \
         FROM query_log ql \
         JOIN app_catalog ac ON ql.app_id = ac.id \
         WHERE ql.time >= datetime('now', printf('-%d hours', ?)) \
           AND (? = '' OR ac.category = ?) \
           AND (? = '' OR ql.status = ?) \
         GROUP BY ac.id \
         ORDER BY total_queries DESC \
         LIMIT ?",
    )
    .bind(hours)
    .bind(&category)
    .bind(&category)
    .bind(&status)
    .bind(&status)
    .bind(limit)
    .fetch_all(&state.db)
    .await?;

    let data: Vec<Value> = rows
        .iter()
        .map(|row| {
            let id: i64 = row.try_get("id").unwrap_or(0);
            let app_name: String = row.try_get("app_name").unwrap_or_default();
            let cat: String = row.try_get("category").unwrap_or_default();
            let icon: String = row.try_get("icon").unwrap_or_default();
            let total_queries: i64 = row.try_get("total_queries").unwrap_or(0);
            let unique_clients: i64 = row.try_get("unique_clients").unwrap_or(0);
            let blocked_queries: i64 = row.try_get("blocked_queries").unwrap_or(0);
            let last_seen: Option<String> = row.try_get("last_seen").unwrap_or(None);

            json!({
                "id": id,
                "app_name": app_name,
                "category": cat,
                "icon": icon,
                "total_queries": total_queries,
                "unique_clients": unique_clients,
                "blocked_queries": blocked_queries,
                "last_seen": last_seen,
            })
        })
        .collect();

    Ok(Json(json!(data)))
}

#[derive(Deserialize)]
pub struct AppTrendParams {
    pub app_id: Option<i64>,
    pub hours: Option<i64>,
}

pub async fn app_trend(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(params): Query<AppTrendParams>,
) -> AppResult<Json<Value>> {
    let app_id = params.app_id.ok_or_else(|| {
        crate::error::AppError::Validation("Missing required parameter: app_id".to_string())
    })?;
    let hours = params.hours.unwrap_or(24).clamp(1, 720);

    // 直接用预计算的 app_id 列过滤，无需 LIKE JOIN。
    let rows = sqlx::query(
        "SELECT strftime('%Y-%m-%dT%H:00:00Z', ql.time) AS hour, \
         COUNT(*) AS total_queries \
         FROM query_log ql \
         WHERE ql.app_id = ? \
           AND ql.time >= datetime('now', printf('-%d hours', ?)) \
         GROUP BY hour \
         ORDER BY hour",
    )
    .bind(app_id)
    .bind(hours)
    .fetch_all(&state.db)
    .await?;

    let data: Vec<Value> = rows
        .iter()
        .map(|row| {
            let hour: String = row.try_get("hour").unwrap_or_default();
            let total_queries: i64 = row.try_get("total_queries").unwrap_or(0);
            json!({
                "hour": hour,
                "total_queries": total_queries,
            })
        })
        .collect();

    Ok(Json(json!(data)))
}

#[derive(Deserialize)]
pub struct CatalogParams {
    pub q: Option<String>,
    pub category: Option<String>,
}

pub async fn list_catalog(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(params): Query<CatalogParams>,
) -> AppResult<Json<Value>> {
    let q = params.q.unwrap_or_default();
    let category = params.category.unwrap_or_default();

    let rows = sqlx::query(
        "SELECT id, app_name, category, icon, vendor, homepage \
         FROM app_catalog \
         WHERE (? = '' OR app_name LIKE '%' || ? || '%') \
           AND (? = '' OR category = ?) \
         ORDER BY category, app_name",
    )
    .bind(&q)
    .bind(&q)
    .bind(&category)
    .bind(&category)
    .fetch_all(&state.db)
    .await?;

    let data: Vec<Value> = rows
        .iter()
        .map(|row| {
            let id: i64 = row.try_get("id").unwrap_or(0);
            let app_name: String = row.try_get("app_name").unwrap_or_default();
            let cat: String = row.try_get("category").unwrap_or_default();
            let icon: String = row.try_get("icon").unwrap_or_default();
            let vendor: Option<String> = row.try_get("vendor").unwrap_or(None);
            let homepage: Option<String> = row.try_get("homepage").unwrap_or(None);
            json!({
                "id": id,
                "app_name": app_name,
                "category": cat,
                "icon": icon,
                "vendor": vendor,
                "homepage": homepage,
            })
        })
        .collect();

    Ok(Json(json!(data)))
}

#[derive(Deserialize)]
pub struct TopDomainsParams {
    pub hours: Option<i64>,
    pub limit: Option<i64>,
    pub status: Option<String>,
}

pub async fn top_domains(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(params): Query<TopDomainsParams>,
) -> AppResult<Json<Value>> {
    let hours = params.hours.unwrap_or(24).clamp(1, 720);
    let limit = params.limit.unwrap_or(20).clamp(1, 100);
    let status = params.status.unwrap_or_default();

    let rows = sqlx::query(
        "SELECT question AS domain, \
         COUNT(*) AS total_queries, \
         COUNT(DISTINCT client_ip) AS unique_clients, \
         SUM(CASE WHEN status = 'blocked' THEN 1 ELSE 0 END) AS blocked_queries, \
         MAX(time) AS last_seen \
         FROM query_log \
         WHERE time >= datetime('now', printf('-%d hours', ?)) \
         GROUP BY question \
         HAVING (? = '') \
             OR (? = 'blocked' AND blocked_queries > 0) \
             OR (? = 'allowed' AND total_queries > blocked_queries) \
         ORDER BY \
             CASE WHEN ? = 'allowed' THEN (total_queries - blocked_queries) \
             WHEN ? = 'blocked' THEN blocked_queries \
             ELSE total_queries END DESC \
         LIMIT ?",
    )
    .bind(hours)
    .bind(&status)
    .bind(&status)
    .bind(&status)
    .bind(&status)
    .bind(&status)
    .bind(limit)
    .fetch_all(&state.db)
    .await?;

    let data: Vec<Value> = rows.iter().map(|row| {
        let domain: String = row.try_get("domain").unwrap_or_default();
        let total_queries: i64 = row.try_get("total_queries").unwrap_or(0);
        let unique_clients: i64 = row.try_get("unique_clients").unwrap_or(0);
        let blocked_queries: i64 = row.try_get("blocked_queries").unwrap_or(0);
        let last_seen: Option<String> = row.try_get("last_seen").unwrap_or(None);
        json!({
            "domain": domain,
            "total_queries": total_queries,
            "unique_clients": unique_clients,
            "blocked_queries": blocked_queries,
            "block_rate": if total_queries > 0 { blocked_queries as f64 / total_queries as f64 * 100.0 } else { 0.0 },
            "last_seen": last_seen,
        })
    }).collect();

    Ok(Json(json!(data)))
}
