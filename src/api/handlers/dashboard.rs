use crate::api::middleware::auth::AuthUser;
use crate::api::AppState;
use crate::error::AppResult;
use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
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

pub async fn get_query_trend(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(params): Query<TrendParams>,
) -> AppResult<Json<Value>> {
    let hours = params.hours.unwrap_or(24).clamp(1, 720);

    // Aggregate query_log by hour over the requested window
    let rows: Vec<(String, i64, i64, i64)> = sqlx::query_as(
        "SELECT
            strftime('%Y-%m-%dT%H:00:00Z', time) as hour,
            COUNT(*) as total,
            SUM(CASE WHEN status = 'blocked' THEN 1 ELSE 0 END) as blocked,
            SUM(CASE WHEN status = 'allowed' THEN 1 ELSE 0 END) as allowed
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
        .map(|(hour, total, blocked, allowed)| {
            json!({
                "time": hour,
                "total": total,
                "blocked": blocked,
                "allowed": allowed,
            })
        })
        .collect();

    Ok(Json(json!(data)))
}
