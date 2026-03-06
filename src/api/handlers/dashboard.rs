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
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

pub async fn get_stats(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(params): Query<TrendParams>,
) -> AppResult<Json<Value>> {
    // Use requested time range (default 24 hours, max 168 hours)
    let hours = params.hours.unwrap_or(24).clamp(1, 720);
    let time_filter = format!("-{} hours", hours);

    // Counts over the requested time window
    let (total,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM query_log WHERE time >= datetime('now', ?)")
            .bind(&time_filter)
            .fetch_one(&state.db)
            .await?;

    let (blocked,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM query_log WHERE status = 'blocked' AND time >= datetime('now', ?)",
    )
    .bind(&time_filter)
    .fetch_one(&state.db)
    .await?;

    let (cached,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM query_log WHERE status = 'cached' AND time >= datetime('now', ?)",
    )
    .bind(&time_filter)
    .fetch_one(&state.db)
    .await?;

    let allowed = total - blocked - cached;
    let block_rate = if total > 0 {
        blocked as f64 / total as f64 * 100.0
    } else {
        0.0
    };

    let (filter_rules,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM custom_rules WHERE is_enabled = 1")
            .fetch_one(&state.db)
            .await?;

    let (filter_lists,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM filter_lists WHERE is_enabled = 1")
            .fetch_one(&state.db)
            .await?;

    // Count unique clients with queries in the requested time window
    let (clients,): (i64,) = sqlx::query_as(
        "SELECT COUNT(DISTINCT client_ip) FROM query_log WHERE time >= datetime('now', ?)",
    )
    .bind(&time_filter)
    .fetch_one(&state.db)
    .await?;

    // Last-week same time window for block-rate trend (week-over-week)
    let offset_days = (hours as f64 / 24.0).round() as i64;
    let week_start_filter = format!("-{} days", offset_days + 7);
    let week_end_filter = format!("-{} days", offset_days);

    let (week_total,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM query_log WHERE time >= datetime('now', ?) AND time < datetime('now', ?)"
    )
    .bind(&week_start_filter)
    .bind(&week_end_filter)
    .fetch_one(&state.db)
            .await?;

    let (week_blocked,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM query_log WHERE status = 'blocked' AND time >= datetime('now', ?) AND time < datetime('now', ?)"
    )
    .bind(&week_start_filter)
    .bind(&week_end_filter)
    .fetch_one(&state.db)
            .await?;

    let last_week_block_rate = if week_total > 0 {
        (week_blocked as f64 / week_total as f64 * 1000.0).round() / 10.0
    } else {
        0.0
    };

    // Last 1 minute QPS
    let (recent_count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM query_log WHERE time >= datetime('now', '-1 minute')")
            .fetch_one(&state.db)
            .await?;
    let qps = (recent_count as f64 / 60.0 * 10.0).round() / 10.0;

    Ok(Json(json!({
        "total_queries": total,
        "blocked_queries": blocked,
        "allowed_queries": allowed,
        "cached_queries": cached,
        "block_rate": (block_rate * 10.0).round() / 10.0,
        "last_week_block_rate": last_week_block_rate,
        "filter_rules": filter_rules,
        "filter_lists": filter_lists,
        "clients": clients,
        "qps": qps,
    })))
}

#[derive(Deserialize)]
pub struct TrendParams {
    pub hours: Option<i64>,
}

pub async fn get_top_blocked_domains(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(params): Query<TrendParams>,
) -> AppResult<Json<Value>> {
    let hours = params.hours.unwrap_or(24).clamp(1, 720);
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT question, COUNT(*) as cnt FROM query_log
         WHERE status = 'blocked' AND time >= datetime('now', printf('-%d hours', ?))
         GROUP BY question ORDER BY cnt DESC LIMIT 10",
    )
    .bind(hours)
    .fetch_all(&state.db)
    .await?;

    let data: Vec<Value> = rows
        .into_iter()
        .map(|(domain, count)| json!({"domain": domain, "count": count}))
        .collect();

    Ok(Json(json!(data)))
}

pub async fn get_top_clients(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(params): Query<TrendParams>,
) -> AppResult<Json<Value>> {
    let hours = params.hours.unwrap_or(24).clamp(1, 720);
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT client_ip, COUNT(*) as cnt FROM query_log
         WHERE time >= datetime('now', printf('-%d hours', ?))
         GROUP BY client_ip ORDER BY cnt DESC LIMIT 10",
    )
    .bind(hours)
    .fetch_all(&state.db)
    .await?;

    let data: Vec<Value> = rows
        .into_iter()
        .map(|(client_ip, count)| json!({"client_ip": client_ip, "count": count}))
        .collect();

    Ok(Json(json!(data)))
}

pub async fn get_top_queried_domains(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(params): Query<TrendParams>,
) -> AppResult<Json<Value>> {
    let hours = params.hours.unwrap_or(24).clamp(1, 720);
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT question, COUNT(*) as cnt FROM query_log
         WHERE time >= datetime('now', printf('-%d hours', ?))
         GROUP BY question ORDER BY cnt DESC LIMIT 10",
    )
    .bind(hours)
    .fetch_all(&state.db)
    .await?;

    let data: Vec<Value> = rows
        .into_iter()
        .map(|(domain, count)| json!({"domain": domain, "count": count}))
        .collect();

    Ok(Json(json!(data)))
}

pub async fn get_query_trend(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(params): Query<TrendParams>,
) -> AppResult<Json<Value>> {
    let hours = params.hours.unwrap_or(24).clamp(1, 720);

    // Aggregate query_log by hour over the requested window
    let rows: Vec<(String, i64, i64, i64, i64)> = sqlx::query_as(
        "SELECT
            strftime('%Y-%m-%dT%H:00:00Z', time) as hour,
            COUNT(*) as total,
            SUM(CASE WHEN status = 'blocked' THEN 1 ELSE 0 END) as blocked,
            SUM(CASE WHEN status = 'allowed' THEN 1 ELSE 0 END) as allowed,
            SUM(CASE WHEN status = 'cached' THEN 1 ELSE 0 END) as cached
         FROM query_log
         WHERE time >= datetime('now', printf('-%d hours', ?))
         GROUP BY strftime('%Y-%m-%d %H', time)
         ORDER BY time ASC",
    )
    .bind(hours)
    .fetch_all(&state.db)
    .await?;

    let data: Vec<Value> = rows
        .into_iter()
        .map(|(hour, total, blocked, allowed, cached)| {
            json!({
                "time": hour,
                "total": total,
                "blocked": blocked,
                "allowed": allowed,
                "cached": cached,
            })
        })
        .collect();

    Ok(Json(json!(data)))
}

#[derive(Deserialize)]
pub struct UpstreamTrendParams {
    pub hours: Option<i64>,
    pub limit: Option<i64>,
}

pub async fn get_upstream_trend(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(params): Query<UpstreamTrendParams>,
) -> AppResult<Json<Value>> {
    let hours = params.hours.unwrap_or(24).clamp(1, 720);
    let limit = params.limit.unwrap_or(10).clamp(1, 50);
    let time_filter = format!("-{} hours", hours);

    // First, find top N upstreams in the time window
    let top_upstreams: Vec<String> = sqlx::query_scalar(
        "SELECT upstream FROM query_log
         WHERE time >= datetime('now', ?) AND upstream IS NOT NULL
         GROUP BY upstream ORDER BY COUNT(*) DESC LIMIT ?",
    )
    .bind(&time_filter)
    .bind(limit)
    .fetch_all(&state.db)
    .await?;

    if top_upstreams.is_empty() {
        return Ok(Json(json!({"data": [], "total_upstreams": 0})));
    }

    // Build IN clause placeholders
    let placeholders = vec!["?"; top_upstreams.len()].join(",");

    // Get hourly aggregates for top upstreams
    let query = format!(
        "SELECT
            strftime('%Y-%m-%dT%H:00:00Z', time) as time,
            upstream,
            COUNT(*) as count
         FROM query_log
         WHERE time >= datetime('now', ?) AND upstream IN ({})
         GROUP BY strftime('%Y-%m-%d %H', time), upstream
         ORDER BY time ASC",
        placeholders
    );

    let mut query_builder = sqlx::query(&query);
    query_builder = query_builder.bind(&time_filter);
    for upstream in &top_upstreams {
        query_builder = query_builder.bind(upstream);
    }

    let rows = query_builder.fetch_all(&state.db).await?;

    // Transform to grouped format — use BTreeMap to keep time slots sorted
    let mut by_time: BTreeMap<String, HashMap<String, i64>> = BTreeMap::new();

    for row in rows {
        let time: String = row.get("time");
        let upstream: String = row.get("upstream");
        let count: i64 = row.get("count");
        by_time.entry(time).or_default().insert(upstream, count);
    }

    // BTreeMap iterates in ascending key order, so time slots are chronological
    let data: Vec<Value> = by_time
        .into_iter()
        .map(|(time, upstreams)| {
            json!({
                "time": time,
                "upstreams": upstreams
            })
        })
        .collect();

    Ok(Json(json!({
        "data": data,
        "total_upstreams": top_upstreams.len()
    })))
}

/// Get upstream distribution (count and percentage) for the time window
pub async fn get_upstream_distribution(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(params): Query<TrendParams>,
) -> AppResult<Json<Value>> {
    let hours = params.hours.unwrap_or(24).clamp(1, 720);
    let time_filter = format!("-{} hours", hours);

    // Get total count for percentage calculation
    let (total_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM query_log
         WHERE time >= datetime('now', ?) AND upstream IS NOT NULL",
    )
    .bind(&time_filter)
    .fetch_one(&state.db)
    .await?;

    if total_count == 0 {
        return Ok(Json(json!([])));
    }

    // Get upstream counts ordered by count DESC
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT upstream, COUNT(*) as cnt FROM query_log
         WHERE time >= datetime('now', ?) AND upstream IS NOT NULL
         GROUP BY upstream ORDER BY cnt DESC",
    )
    .bind(&time_filter)
    .fetch_all(&state.db)
    .await?;

    let data: Vec<Value> = rows
        .into_iter()
        .map(|(upstream, count)| {
            let percentage = (count as f64 / total_count as f64 * 100.0 * 10.0).round() / 10.0;
            json!({
                "upstream": upstream,
                "count": count,
                "percentage": percentage
            })
        })
        .collect();

    Ok(Json(json!(data)))
}
