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
         MAX(ql.time)::text AS last_seen \
         FROM query_log ql \
         JOIN app_catalog ac ON ql.app_id = ac.id \
         WHERE ql.time >= NOW() - ($1 * INTERVAL '1 hour') \
           AND ($2 = '' OR ac.category = $3) \
           AND ($4 = '' OR ql.status = $5) \
         GROUP BY ac.id \
         ORDER BY total_queries DESC \
         LIMIT $6",
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
        "SELECT TO_CHAR(date_trunc('hour', ql.time), 'YYYY-MM-DD\"T\"HH24:00:00Z') AS hour, \
         COUNT(*) AS total_queries \
         FROM query_log ql \
         WHERE ql.app_id = $1 \
           AND ql.time >= NOW() - ($2 * INTERVAL '1 hour') \
         GROUP BY date_trunc('hour', ql.time) \
         ORDER BY date_trunc('hour', ql.time)",
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
         WHERE ($1 = '' OR app_name LIKE '%' || $2 || '%') \
           AND ($3 = '' OR category = $4) \
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
         MAX(time)::text AS last_seen \
         FROM query_log \
         WHERE time >= NOW() - ($1 * INTERVAL '1 hour') \
         GROUP BY question \
         HAVING ($2 = '') \
             OR ($3 = 'blocked' AND SUM(CASE WHEN status = 'blocked' THEN 1 ELSE 0 END) > 0) \
             OR ($4 = 'allowed' AND COUNT(*) > SUM(CASE WHEN status = 'blocked' THEN 1 ELSE 0 END)) \
         ORDER BY \
             CASE WHEN $5 = 'allowed' THEN (COUNT(*) - SUM(CASE WHEN status = 'blocked' THEN 1 ELSE 0 END)) \
             WHEN $6 = 'blocked' THEN SUM(CASE WHEN status = 'blocked' THEN 1 ELSE 0 END) \
             ELSE COUNT(*) END DESC \
         LIMIT $7",
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

pub async fn get_anomalies(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
) -> AppResult<Json<Value>> {
    // 查询过去 7 天（不含当前小时）每个 client_ip 的每小时请求数
    let history_rows = sqlx::query(
        "SELECT client_ip, TO_CHAR(time, 'YYYY-MM-DD\"T\"HH24:00:00') as hour, COUNT(*) as cnt \
         FROM query_log \
         WHERE time >= NOW() - INTERVAL '7 days' \
           AND time < date_trunc('hour', NOW()) \
         GROUP BY client_ip, hour",
    )
    .fetch_all(&state.db)
    .await?;

    // 在 Rust 中按 client_ip 分组，计算均值和标准差
    use std::collections::HashMap;
    let mut history: HashMap<String, Vec<f64>> = HashMap::new();
    for row in &history_rows {
        let ip: String = row.try_get("client_ip").unwrap_or_default();
        let cnt: i64 = row.try_get("cnt").unwrap_or(0);
        history.entry(ip).or_default().push(cnt as f64);
    }

    // 只保留至少有 24 个小时数据点的 client
    let stats: HashMap<String, (f64, f64)> = history
        .into_iter()
        .filter(|(_, counts)| counts.len() >= 24)
        .map(|(ip, counts)| {
            let n = counts.len() as f64;
            let sum: f64 = counts.iter().sum();
            let sq_sum: f64 = counts.iter().map(|x| x * x).sum();
            let mean = sum / n;
            let variance = (sq_sum / n - mean * mean).max(0.0);
            let stddev = variance.sqrt();
            (ip, (mean, stddev))
        })
        .collect();

    // 查询当前小时每个 client_ip 的请求数
    let current_rows = sqlx::query(
        "SELECT client_ip, COUNT(*) as cnt \
         FROM query_log \
         WHERE time >= date_trunc('hour', NOW()) \
         GROUP BY client_ip",
    )
    .fetch_all(&state.db)
    .await?;

    // 标记异常：current_count > mean + 2.0 * stddev
    let mut anomalies: Vec<Value> = Vec::new();
    for row in &current_rows {
        let ip: String = row.try_get("client_ip").unwrap_or_default();
        let current_count: i64 = row.try_get("cnt").unwrap_or(0);

        if let Some((mean, stddev)) = stats.get(&ip) {
            let threshold = mean + 2.0 * stddev;
            if current_count as f64 > threshold {
                let sigma = if *stddev > 0.0 {
                    (current_count as f64 - mean) / stddev
                } else {
                    0.0
                };
                anomalies.push(json!({
                    "client_ip": ip,
                    "current_count": current_count,
                    "mean": (mean * 100.0).round() / 100.0,
                    "stddev": (stddev * 100.0).round() / 100.0,
                    "sigma": (sigma * 100.0).round() / 100.0,
                }));
            }
        }
    }

    // 按 sigma 降序排列
    anomalies.sort_by(|a, b| {
        let sa = a["sigma"].as_f64().unwrap_or(0.0);
        let sb = b["sigma"].as_f64().unwrap_or(0.0);
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });

    // 为每个异常设备自动创建 Alert（1小时冷却，避免重复告警）
    for anomaly in &anomalies {
        let ip = anomaly["client_ip"].as_str().unwrap_or("");
        let current_count = anomaly["current_count"].as_i64().unwrap_or(0);
        let mean = anomaly["mean"].as_f64().unwrap_or(0.0);
        let sigma = anomaly["sigma"].as_f64().unwrap_or(0.0);

        let existing: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM alerts \
             WHERE alert_type = 'anomaly_detection' \
               AND client_id = $1 \
               AND created_at > NOW() - INTERVAL '1 hour'",
        )
        .bind(ip)
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);

        if existing == 0 {
            let alert_id = uuid::Uuid::new_v4().to_string();
            let message = format!(
                "设备 {} 请求量异常：当前 {} 次/小时，均值 {:.1}，偏差 {:.1}σ",
                ip, current_count, mean, sigma
            );
            let _ = sqlx::query(
                "INSERT INTO alerts (id, alert_type, client_id, message, is_read, created_at) \
                 VALUES ($1, 'anomaly_detection', $2, $3, 0, NOW())",
            )
            .bind(&alert_id)
            .bind(ip)
            .bind(&message)
            .execute(&state.db)
            .await;
        }
    }

    Ok(Json(json!(anomalies)))
}
