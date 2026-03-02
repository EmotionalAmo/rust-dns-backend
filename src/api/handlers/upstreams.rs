use axum::{
    extract::{Path, State},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

use crate::api::middleware::auth::AuthUser;
use crate::api::middleware::client_ip::ClientIp;
use crate::api::middleware::rbac::AdminUser;
use crate::api::AppState;
use crate::error::{AppError, AppResult};

#[derive(Debug, Deserialize)]
pub struct CreateUpstreamRequest {
    pub name: String,
    pub addresses: Vec<String>,
    #[serde(default = "default_priority")]
    pub priority: i32,
    #[serde(default = "default_health_check_interval")]
    pub health_check_interval: i64,
    #[serde(default = "default_health_check_timeout")]
    pub health_check_timeout: i64,
    #[serde(default = "default_failover_threshold")]
    pub failover_threshold: i64,
}

fn default_priority() -> i32 {
    1
}
fn default_health_check_interval() -> i64 {
    30
}
fn default_health_check_timeout() -> i64 {
    5
}
fn default_failover_threshold() -> i64 {
    3
}

#[derive(Debug, Deserialize)]
pub struct UpdateUpstreamRequest {
    pub name: Option<String>,
    pub addresses: Option<Vec<String>>,
    pub priority: Option<i32>,
    pub is_active: Option<bool>,
    pub health_check_enabled: Option<bool>,
    pub failover_enabled: Option<bool>,
    pub health_check_interval: Option<i64>,
    pub health_check_timeout: Option<i64>,
    pub failover_threshold: Option<i64>,
}

// Local struct to handle the 18-column query (sqlx tuples are capped at 16)
#[derive(sqlx::FromRow)]
struct UpstreamRow {
    id: String,
    name: String,
    addresses: String,
    priority: i32,
    is_active: i64,
    health_check_enabled: i64,
    failover_enabled: i64,
    health_check_interval: i64,
    health_check_timeout: i64,
    failover_threshold: i64,
    health_status: String,
    last_health_check_at: Option<String>,
    last_failover_at: Option<String>,
    created_at: String,
    updated_at: String,
    last_latency_ms: Option<i64>,
    avg_30m_ms: Option<i64>,
    avg_60m_ms: Option<i64>,
}

pub async fn list(State(state): State<Arc<AppState>>, _auth: AuthUser) -> AppResult<Json<Value>> {
    let rows: Vec<UpstreamRow> = sqlx::query_as(
        "SELECT u.id, u.name, u.addresses, u.priority, u.is_active, u.health_check_enabled,
                u.failover_enabled, u.health_check_interval, u.health_check_timeout, u.failover_threshold,
                u.health_status, u.last_health_check_at, u.last_failover_at, u.created_at, u.updated_at,
                (SELECT latency_ms FROM upstream_latency_log
                 WHERE upstream_id = u.id ORDER BY id DESC LIMIT 1) AS last_latency_ms,
                (SELECT CAST(AVG(latency_ms) AS INTEGER) FROM upstream_latency_log
                 WHERE upstream_id = u.id AND success = 1
                   AND checked_at >= datetime('now', '-30 minutes')) AS avg_30m_ms,
                (SELECT CAST(AVG(latency_ms) AS INTEGER) FROM upstream_latency_log
                 WHERE upstream_id = u.id AND success = 1
                   AND checked_at >= datetime('now', '-60 minutes')) AS avg_60m_ms
         FROM dns_upstreams u ORDER BY u.priority ASC, u.name ASC"
    )
    .fetch_all(&state.db)
    .await?;

    let data: Vec<Value> = rows
        .into_iter()
        .map(|r| {
            // P1-4 fix：JSON 解析失败记录警告，不再静默返回空列表
            let addresses_vec: Vec<String> =
                serde_json::from_str(&r.addresses).unwrap_or_else(|e| {
                    tracing::warn!("Upstream {} has invalid addresses JSON: {}", r.id, e);
                    Vec::new()
                });
            json!({
                "id": r.id,
                "name": r.name,
                "addresses": addresses_vec,
                "priority": r.priority,
                "is_active": r.is_active == 1,
                "health_check_enabled": r.health_check_enabled == 1,
                "failover_enabled": r.failover_enabled == 1,
                "health_check_interval": r.health_check_interval,
                "health_check_timeout": r.health_check_timeout,
                "failover_threshold": r.failover_threshold,
                "health_status": r.health_status,
                "last_health_check_at": r.last_health_check_at,
                "last_failover_at": r.last_failover_at,
                "created_at": r.created_at,
                "updated_at": r.updated_at,
                "last_latency_ms": r.last_latency_ms,
                "avg_latency_30m_ms": r.avg_30m_ms,
                "avg_latency_60m_ms": r.avg_60m_ms,
            })
        })
        .collect();

    let total = data.len();
    Ok(Json(json!({ "data": data, "total": total })))
}

type UpstreamDetailRow = (
    String,
    String,
    String,
    i32,
    i64,
    i64,
    i64,
    i64,
    i64,
    i64,
    String,
    Option<String>,
    Option<String>,
    String,
    String,
);

pub async fn get(
    State(state): State<Arc<AppState>>,
    #[allow(unused_variables)] ClientIp(ip): ClientIp,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    let row: Option<UpstreamDetailRow> = sqlx::query_as(
        "SELECT id, name, addresses, priority, is_active, health_check_enabled,
                failover_enabled, health_check_interval, health_check_timeout, failover_threshold,
                health_status, last_health_check_at, last_failover_at, created_at, updated_at
         FROM dns_upstreams WHERE id = ?",
    )
    .bind(&id)
    .fetch_optional(&state.db)
    .await?;

    let (
        id,
        name,
        addresses,
        priority,
        is_active,
        health_check_enabled,
        failover_enabled,
        health_check_interval,
        health_check_timeout,
        failover_threshold,
        health_status,
        last_health_check_at,
        last_failover_at,
        created_at,
        updated_at,
    ) = row.ok_or_else(|| AppError::NotFound(format!("Upstream {} not found", id)))?;

    // P1-4 fix：记录解析警告
    let addresses_vec: Vec<String> = serde_json::from_str(&addresses).unwrap_or_else(|e| {
        tracing::warn!("Upstream {} has invalid addresses JSON: {}", id, e);
        Vec::new()
    });

    Ok(Json(json!({
        "id": id,
        "name": name,
        "addresses": addresses_vec,
        "priority": priority,
        "is_active": is_active == 1,
        "health_check_enabled": health_check_enabled == 1,
        "failover_enabled": failover_enabled == 1,
        "health_check_interval": health_check_interval,
        "health_check_timeout": health_check_timeout,
        "failover_threshold": failover_threshold,
        "health_status": health_status,
        "last_health_check_at": last_health_check_at,
        "last_failover_at": last_failover_at,
        "created_at": created_at,
        "updated_at": updated_at,
    })))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    #[allow(unused_variables)] ClientIp(ip): ClientIp,
    _admin: AdminUser,
    Json(body): Json<CreateUpstreamRequest>,
) -> AppResult<Json<Value>> {
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::Validation(
            "Upstream name cannot be empty".to_string(),
        ));
    }
    if body.addresses.is_empty() {
        return Err(AppError::Validation(
            "At least one address is required".to_string(),
        ));
    }

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let addresses = serde_json::to_string(&body.addresses)?;
    let failover_threshold = body.failover_threshold;

    sqlx::query(
        "INSERT INTO dns_upstreams
            (id, name, addresses, priority, is_active, health_check_enabled,
             failover_enabled, health_check_interval, health_check_timeout,
             failover_threshold, health_status, created_at, updated_at)
         VALUES (?, ?, ?, ?, 1, 1, 1, ?, ?, ?, 'unknown', ?, ?)",
    )
    .bind(&id)
    .bind(&name)
    .bind(&addresses)
    .bind(body.priority)
    .bind(body.health_check_interval)
    .bind(body.health_check_timeout)
    .bind(failover_threshold)
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await?;

    // Hot-reload the upstream pool
    if let Err(e) = state.dns_handler.reload_upstreams().await {
        tracing::error!(
            "Failed to reload upstream pool after creating upstream: {}",
            e
        );
    }

    Ok(Json(json!({
        "id": id,
        "name": name,
        "addresses": body.addresses,
        "priority": body.priority,
        "is_active": true,
        "health_check_enabled": true,
        "failover_enabled": true,
        "health_check_interval": body.health_check_interval,
        "health_check_timeout": body.health_check_timeout,
        "failover_threshold": failover_threshold,
        "health_status": "unknown",
        "last_health_check_at": None::<Option<String>>,
        "last_failover_at": None::<Option<String>>,
        "created_at": now,
        "updated_at": now,
    })))
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    #[allow(unused_variables)] ClientIp(ip): ClientIp,
    _admin: AdminUser,
    Path(id): Path<String>,
    Json(body): Json<UpdateUpstreamRequest>,
) -> AppResult<Json<Value>> {
    // Check if upstream exists
    let existing: Option<UpstreamDetailRow> = sqlx::query_as(
        "SELECT id, name, addresses, priority, is_active, health_check_enabled,
                failover_enabled, health_check_interval, health_check_timeout, failover_threshold,
                health_status, last_health_check_at, last_failover_at, created_at, updated_at
         FROM dns_upstreams WHERE id = ?",
    )
    .bind(&id)
    .fetch_optional(&state.db)
    .await?;

    let (
        _,
        old_name,
        old_addresses,
        old_priority,
        old_is_active,
        old_health_check_enabled,
        old_failover_enabled,
        old_health_check_interval,
        old_health_check_timeout,
        old_failover_threshold,
        old_health_status,
        old_last_health_check_at,
        old_last_failover_at,
        old_created_at,
        _old_updated_at,
    ) = existing.ok_or_else(|| AppError::NotFound(format!("Upstream {} not found", id)))?;

    let name = body.name.unwrap_or(old_name);
    let addresses = if let Some(a) = body.addresses {
        serde_json::to_string(&a)
            .map_err(|e| AppError::Internal(format!("Failed to serialize addresses: {}", e)))?
    } else {
        old_addresses
    };
    let priority = body.priority.unwrap_or(old_priority);
    let is_active = body
        .is_active
        .map(|b| if b { 1 } else { 0 })
        .unwrap_or(old_is_active);
    let health_check_enabled = body
        .health_check_enabled
        .map(|b| if b { 1 } else { 0 })
        .unwrap_or(old_health_check_enabled);
    let failover_enabled = body
        .failover_enabled
        .map(|b| if b { 1 } else { 0 })
        .unwrap_or(old_failover_enabled);
    let health_check_interval = body
        .health_check_interval
        .unwrap_or(old_health_check_interval);
    let health_check_timeout = body
        .health_check_timeout
        .unwrap_or(old_health_check_timeout);
    let failover_threshold = body.failover_threshold.unwrap_or(old_failover_threshold);

    let now = chrono::Utc::now().to_rfc3339();
    let addresses_vec: Vec<String> = serde_json::from_str(&addresses).unwrap_or_else(|e| {
        tracing::warn!(
            "Upstream {} has invalid addresses JSON in update path: {}",
            id,
            e
        );
        Vec::new()
    });

    sqlx::query(
        "UPDATE dns_upstreams
         SET name = ?, addresses = ?, priority = ?, is_active = ?,
             health_check_enabled = ?, failover_enabled = ?,
             health_check_interval = ?, health_check_timeout = ?, failover_threshold = ?,
             updated_at = ?
         WHERE id = ?",
    )
    .bind(&name)
    .bind(&addresses)
    .bind(priority)
    .bind(is_active)
    .bind(health_check_enabled)
    .bind(failover_enabled)
    .bind(health_check_interval)
    .bind(health_check_timeout)
    .bind(failover_threshold)
    .bind(&now)
    .bind(&id)
    .execute(&state.db)
    .await?;

    // Hot-reload the upstream pool
    if let Err(e) = state.dns_handler.reload_upstreams().await {
        tracing::error!(
            "Failed to reload upstream pool after updating upstream: {}",
            e
        );
    }

    Ok(Json(json!({
        "id": id,
        "name": name,
        "addresses": addresses_vec,
        "priority": priority,
        "is_active": is_active == 1,
        "health_check_enabled": health_check_enabled == 1,
        "failover_enabled": failover_enabled == 1,
        "health_check_interval": health_check_interval,
        "health_check_timeout": health_check_timeout,
        "failover_threshold": failover_threshold,
        "health_status": old_health_status,
        "last_health_check_at": old_last_health_check_at,
        "last_failover_at": old_last_failover_at,
        "created_at": old_created_at,
        "updated_at": now,
    })))
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    #[allow(unused_variables)] ClientIp(ip): ClientIp,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    let result = sqlx::query("DELETE FROM dns_upstreams WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("Upstream {} not found", id)));
    }

    // Hot-reload the upstream pool
    if let Err(e) = state.dns_handler.reload_upstreams().await {
        tracing::error!(
            "Failed to reload upstream pool after deleting upstream: {}",
            e
        );
    }

    Ok(Json(json!({"success": true})))
}

pub async fn test(
    State(state): State<Arc<AppState>>,
    #[allow(unused_variables)] ClientIp(ip): ClientIp,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    let row: Option<(String, String, i64)> = sqlx::query_as(
        "SELECT id, addresses, health_check_timeout FROM dns_upstreams WHERE id = ?",
    )
    .bind(&id)
    .fetch_optional(&state.db)
    .await?;

    let (_, addresses, timeout) =
        row.ok_or_else(|| AppError::NotFound(format!("Upstream {} not found", id)))?;

    let addresses_vec: Vec<String> = serde_json::from_str(&addresses)
        .map_err(|e| AppError::Internal(format!("Invalid addresses format: {}", e)))?;

    if addresses_vec.is_empty() {
        return Ok(Json(json!({
            "success": false,
            "latency_ms": 0,
            "error": "No addresses configured"
        })));
    }

    let timeout_sec = std::time::Duration::from_secs(timeout as u64);
    let start = std::time::Instant::now();
    let result = test_dns_connectivity(&addresses_vec[0], timeout_sec).await;
    let latency = start.elapsed().as_millis() as i64;
    let success = result.is_ok();
    let error_msg = result.as_ref().err().map(|e| e.to_string());

    // Persist latency sample and update health status
    let now = chrono::Utc::now().to_rfc3339();
    let _ = sqlx::query(
        "INSERT INTO upstream_latency_log (upstream_id, latency_ms, success, checked_at) VALUES (?, ?, ?, ?)"
    )
    .bind(&id)
    .bind(latency)
    .bind(if success { 1i64 } else { 0i64 })
    .bind(&now)
    .execute(&state.db)
    .await;

    let new_status = if success { "healthy" } else { "degraded" };
    let _ = sqlx::query(
        "UPDATE dns_upstreams SET health_status = ?, last_health_check_at = ?, updated_at = ? WHERE id = ?"
    )
    .bind(new_status)
    .bind(&now)
    .bind(&now)
    .bind(&id)
    .execute(&state.db)
    .await;

    Ok(Json(json!({
        "success": success,
        "latency_ms": latency,
        "error": error_msg
    })))
}

pub async fn trigger_failover(
    State(state): State<Arc<AppState>>,
    #[allow(unused_variables)] ClientIp(ip): ClientIp,
    _admin: AdminUser,
) -> AppResult<Json<Value>> {
    let rows: Vec<(String, String, String, i32, String)> = sqlx::query_as(
        "SELECT id, name, addresses, priority, health_status
         FROM dns_upstreams
         WHERE is_active = 1
         ORDER BY priority ASC",
    )
    .fetch_all(&state.db)
    .await?;

    if rows.is_empty() {
        return Ok(Json(json!({
            "success": false,
            "new_upstream_id": null,
            "message": "No active upstreams configured"
        })));
    }

    // Find first healthy upstream
    let new_upstream = rows.iter().find(|(_, _, _, _, status)| status == "healthy");

    if let Some((id, name, _, _, _)) = new_upstream {
        // Log the failover
        let log_id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO upstream_failover_log (id, upstream_id, action, reason, timestamp)
             VALUES (?, ?, 'failover_triggered', 'Manual failover triggered by user', ?)",
        )
        .bind(&log_id)
        .bind(id)
        .bind(&now)
        .execute(&state.db)
        .await?;

        tracing::info!("Manual failover to upstream: {} ({})", id, name);
        Ok(Json(json!({
            "success": true,
            "new_upstream_id": id,
            "message": format!("Switched to {}", name)
        })))
    } else {
        Ok(Json(json!({
            "success": false,
            "new_upstream_id": null,
            "message": "No healthy upstreams available for failover"
        })))
    }
}

pub async fn failover_log(
    State(state): State<Arc<AppState>>,
    #[allow(unused_variables)] ClientIp(ip): ClientIp,
    _auth: AuthUser,
) -> AppResult<Json<Value>> {
    let rows: Vec<(String, String, String, Option<String>, String)> = sqlx::query_as(
        "SELECT id, upstream_id, action, reason, timestamp
         FROM upstream_failover_log
         ORDER BY timestamp DESC
         LIMIT 100",
    )
    .fetch_all(&state.db)
    .await?;

    let data: Vec<Value> = rows
        .into_iter()
        .map(|(id, upstream_id, action, reason, timestamp)| {
            json!({
                "id": id,
                "upstream_id": upstream_id,
                "action": action,
                "reason": reason,
                "timestamp": timestamp,
            })
        })
        .collect();

    Ok(Json(json!({ "data": data, "total": data.len() })))
}

/// Simple DNS connectivity test using hickory-resolver
async fn test_dns_connectivity(addr: &str, timeout: std::time::Duration) -> anyhow::Result<()> {
    use hickory_resolver::config::{NameServerConfig, Protocol, ResolverConfig};
    use hickory_resolver::TokioAsyncResolver;

    // Parse address (format: "ip:port" or "ip")
    let (ip, port) = if addr.contains(':') {
        let parts: Vec<&str> = addr.rsplitn(2, ':').collect();
        (parts[1], parts[0].parse::<u16>()?)
    } else {
        (addr, 53)
    };

    let ip_addr = ip.parse::<std::net::IpAddr>()?;

    let mut config = ResolverConfig::new();
    config.add_name_server(NameServerConfig {
        socket_addr: (ip_addr, port).into(),
        protocol: Protocol::Udp,
        trust_negative_responses: false,
        tls_config: None,
        bind_addr: None,
        tls_dns_name: None,
    });

    let opts = hickory_resolver::config::ResolverOpts::default();
    let resolver = TokioAsyncResolver::tokio(config, opts);

    // Try a simple lookup with timeout
    let _ = tokio::time::timeout(timeout, resolver.lookup_ip("example.com.")).await??;

    Ok(())
}

pub async fn get_health(
    State(state): State<Arc<AppState>>,
    #[allow(unused_variables)] ClientIp(ip): ClientIp,
    _auth: AuthUser,
) -> AppResult<Json<Value>> {
    // Return cached upstream health states
    let mut data = std::collections::HashMap::new();
    for entry in state.upstream_health.iter() {
        data.insert(entry.key().clone(), entry.value().clone());
    }
    Ok(Json(json!({ "data": data })))
}
