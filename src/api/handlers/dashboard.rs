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
use std::collections::{BTreeMap, BTreeSet, HashMap};
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

pub async fn get_latency_stats(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(params): Query<TrendParams>,
) -> AppResult<Json<Value>> {
    let hours = params.hours.unwrap_or(24).clamp(1, 720);
    let time_filter = format!("-{} hours", hours);

    let rows: Vec<(i64,)> = sqlx::query_as(
        "SELECT elapsed_ns FROM query_log
         WHERE upstream IS NOT NULL AND time >= datetime('now', ?)
         ORDER BY id DESC LIMIT 5000",
    )
    .bind(&time_filter)
    .fetch_all(&state.db)
    .await?;

    let sample_count = rows.len();

    if sample_count == 0 {
        return Ok(Json(json!({
            "p50_ms": null,
            "p95_ms": null,
            "p99_ms": null,
            "sample_count": 0
        })));
    }

    let mut values: Vec<i64> = rows.into_iter().map(|(ns,)| ns).collect();
    values.sort_unstable();

    let percentile = |p: f64| -> f64 {
        let idx = ((p / 100.0) * (sample_count as f64 - 1.0)).round() as usize;
        let idx = idx.min(sample_count - 1);
        (values[idx] as f64 / 1_000_000.0 * 10.0).round() / 10.0
    };

    let p50 = percentile(50.0);
    let p95 = percentile(95.0);
    let p99 = percentile(99.0);

    Ok(Json(json!({
        "p50_ms": p50,
        "p95_ms": p95,
        "p99_ms": p99,
        "sample_count": sample_count
    })))
}

/// Get latency trend (P50/P95 per time bucket) for the time window.
/// Bucket granularity: 1h for ≤24h, 6h for ≤168h, 24h for >168h.
pub async fn get_latency_trend(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(params): Query<TrendParams>,
) -> AppResult<Json<Value>> {
    let hours = params.hours.unwrap_or(24).clamp(1, 720);
    let time_filter = format!("-{} hours", hours);

    // Fetch (hourly_bucket, elapsed_ns) pairs, capped at 30000 rows
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT strftime('%Y-%m-%dT%H:00:00Z', time) as bucket, elapsed_ns
         FROM query_log
         WHERE upstream IS NOT NULL AND time >= datetime('now', ?)
         ORDER BY bucket ASC
         LIMIT 30000",
    )
    .bind(&time_filter)
    .fetch_all(&state.db)
    .await?;

    if rows.is_empty() {
        return Ok(Json(json!([])));
    }

    // Determine bucket granularity
    let bucket_hours: u32 = if hours <= 24 {
        1
    } else if hours <= 168 {
        6
    } else {
        24
    };

    // Group elapsed_ns values by merged bucket
    let mut buckets: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    for (hourly_bucket, elapsed_ns) in rows {
        let merged = if bucket_hours == 1 {
            hourly_bucket
        } else {
            // Parse hour from "2026-03-07T14:00:00Z" (positions 11..13)
            let hour: u32 = hourly_bucket
                .get(11..13)
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let rounded = (hour / bucket_hours) * bucket_hours;
            format!("{}T{:02}:00:00Z", &hourly_bucket[..10], rounded)
        };
        buckets.entry(merged).or_default().push(elapsed_ns);
    }

    // Compute P50/P95 per bucket
    let percentile = |sorted: &[i64], p: f64| -> f64 {
        let n = sorted.len();
        let idx = ((p / 100.0) * (n as f64 - 1.0)).round() as usize;
        let idx = idx.min(n - 1);
        (sorted[idx] as f64 / 1_000_000.0 * 10.0).round() / 10.0
    };

    let data: Vec<Value> = buckets
        .into_iter()
        .map(|(time, mut values)| {
            values.sort_unstable();
            let count = values.len();
            let p50 = percentile(&values, 50.0);
            let p95 = percentile(&values, 95.0);
            json!({
                "time": time,
                "p50_ms": p50,
                "p95_ms": p95,
                "sample_count": count
            })
        })
        .collect();

    Ok(Json(json!(data)))
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

/// Get upstream availability history (success rate %) bucketed by hour for the time window.
/// Returns { data: [{time, upstreams: {name: pct}}], upstreams: [name, ...] }
pub async fn get_upstream_health_history(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(params): Query<TrendParams>,
) -> AppResult<Json<Value>> {
    let hours = params.hours.unwrap_or(24).clamp(1, 720);
    let time_filter = format!("-{} hours", hours);

    let rows: Vec<(String, String, f64)> = sqlx::query_as(
        "SELECT u.name,
                strftime('%Y-%m-%dT%H:00:00Z', l.checked_at) as bucket,
                CAST(SUM(l.success) * 100.0 / COUNT(*) AS REAL) as availability
         FROM upstream_latency_log l
         JOIN dns_upstreams u ON u.id = l.upstream_id
         WHERE l.checked_at >= datetime('now', ?)
         GROUP BY u.name, strftime('%Y-%m-%d %H', l.checked_at)
         ORDER BY bucket ASC",
    )
    .bind(&time_filter)
    .fetch_all(&state.db)
    .await?;

    if rows.is_empty() {
        return Ok(Json(json!({ "data": [], "upstreams": [] })));
    }

    let mut upstream_names: BTreeSet<String> = BTreeSet::new();
    let mut by_time: BTreeMap<String, HashMap<String, f64>> = BTreeMap::new();

    for (name, bucket, availability) in rows {
        upstream_names.insert(name.clone());
        by_time
            .entry(bucket)
            .or_default()
            .insert(name, (availability * 10.0).round() / 10.0);
    }

    let upstreams: Vec<String> = upstream_names.into_iter().collect();

    let data: Vec<Value> = by_time
        .into_iter()
        .map(|(time, upstream_map)| json!({ "time": time, "upstreams": upstream_map }))
        .collect();

    Ok(Json(json!({ "data": data, "upstreams": upstreams })))
}
