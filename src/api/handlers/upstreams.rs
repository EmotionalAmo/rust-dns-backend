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
        "SELECT u.id, u.name, u.addresses, CAST(u.priority AS INTEGER) as priority,
                u.is_active::bigint as is_active, u.health_check_enabled::bigint as health_check_enabled,
                u.failover_enabled::bigint as failover_enabled,
                u.health_check_interval, u.health_check_timeout, u.failover_threshold,
                u.health_status,
                u.last_health_check_at::text as last_health_check_at,
                u.last_failover_at::text as last_failover_at,
                u.created_at::text as created_at,
                u.updated_at::text as updated_at,
                (SELECT CAST(latency_ms AS BIGINT) FROM upstream_latency_log
                 WHERE upstream_id = u.id ORDER BY id DESC LIMIT 1) AS last_latency_ms,
                (SELECT CAST(AVG(latency_ms) AS BIGINT) FROM upstream_latency_log
                 WHERE upstream_id = u.id AND success = 1
                   AND checked_at >= NOW() - INTERVAL '30 minutes') AS avg_30m_ms,
                (SELECT CAST(AVG(latency_ms) AS BIGINT) FROM upstream_latency_log
                 WHERE upstream_id = u.id AND success = 1
                   AND checked_at >= NOW() - INTERVAL '60 minutes') AS avg_60m_ms
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
        "SELECT id, name, addresses, CAST(priority AS INTEGER) as priority,
                is_active::bigint as is_active, health_check_enabled::bigint as health_check_enabled,
                failover_enabled::bigint as failover_enabled,
                health_check_interval, health_check_timeout, failover_threshold,
                health_status,
                last_health_check_at::text as last_health_check_at,
                last_failover_at::text as last_failover_at,
                created_at::text as created_at,
                updated_at::text as updated_at
         FROM dns_upstreams WHERE id = $1",
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
    ClientIp(ip): ClientIp,
    admin: AdminUser,
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
         VALUES ($1, $2, $3, $4, 1, 1, 1, $5, $6, $7, 'unknown', $8, $9)",
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

    crate::db::audit::log_action(
        state.db.clone(),
        admin.0.sub.clone(),
        admin.0.username.clone(),
        "create",
        "upstream",
        Some(id.clone()),
        Some(name.clone()),
        ip,
    );

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
    ClientIp(ip): ClientIp,
    admin: AdminUser,
    Path(id): Path<String>,
    Json(body): Json<UpdateUpstreamRequest>,
) -> AppResult<Json<Value>> {
    // Check if upstream exists
    let existing: Option<UpstreamDetailRow> = sqlx::query_as(
        "SELECT id, name, addresses, CAST(priority AS INTEGER) as priority,
                is_active::bigint as is_active, health_check_enabled::bigint as health_check_enabled,
                failover_enabled::bigint as failover_enabled,
                health_check_interval, health_check_timeout, failover_threshold,
                health_status,
                last_health_check_at::text as last_health_check_at,
                last_failover_at::text as last_failover_at,
                created_at::text as created_at,
                updated_at::text as updated_at
         FROM dns_upstreams WHERE id = $1",
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
    let is_active: bool = body.is_active.unwrap_or(old_is_active == 1);
    let health_check_enabled: bool = body
        .health_check_enabled
        .unwrap_or(old_health_check_enabled == 1);
    let failover_enabled: bool = body.failover_enabled.unwrap_or(old_failover_enabled == 1);
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
         SET name = $1, addresses = $2, priority = $3, is_active = $4,
             health_check_enabled = $5, failover_enabled = $6,
             health_check_interval = $7, health_check_timeout = $8, failover_threshold = $9,
             updated_at = $10
         WHERE id = $11",
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

    crate::db::audit::log_action(
        state.db.clone(),
        admin.0.sub.clone(),
        admin.0.username.clone(),
        "update",
        "upstream",
        Some(id.clone()),
        Some(name.clone()),
        ip,
    );

    Ok(Json(json!({
        "id": id,
        "name": name,
        "addresses": addresses_vec,
        "priority": priority,
        "is_active": is_active,
        "health_check_enabled": health_check_enabled,
        "failover_enabled": failover_enabled,
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
    ClientIp(ip): ClientIp,
    admin: AdminUser,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    let result = sqlx::query("DELETE FROM dns_upstreams WHERE id = $1")
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

    crate::db::audit::log_action(
        state.db.clone(),
        admin.0.sub.clone(),
        admin.0.username.clone(),
        "delete",
        "upstream",
        Some(id.clone()),
        None,
        ip,
    );

    Ok(Json(json!({"success": true})))
}

pub async fn test(
    State(state): State<Arc<AppState>>,
    #[allow(unused_variables)] ClientIp(ip): ClientIp,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    let row: Option<(String, String, i64)> = sqlx::query_as(
        "SELECT id, addresses, health_check_timeout FROM dns_upstreams WHERE id = $1",
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
        "INSERT INTO upstream_latency_log (upstream_id, latency_ms, success, checked_at) VALUES ($1, $2, $3, $4)"
    )
    .bind(&id)
    .bind(latency)
    .bind(if success { 1i64 } else { 0i64 })
    .bind(&now)
    .execute(&state.db)
    .await;

    let new_status = if success { "healthy" } else { "degraded" };
    let _ = sqlx::query(
        "UPDATE dns_upstreams SET health_status = $1, last_health_check_at = $2, updated_at = $3 WHERE id = $4"
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
        "SELECT id, name, addresses, CAST(priority AS INTEGER), health_status
         FROM dns_upstreams
         WHERE is_active = true
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
             VALUES ($1, $2, 'failover_triggered', 'Manual failover triggered by user', $3)",
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
    let rows: Vec<(String, String, String, String, Option<String>, String)> = sqlx::query_as(
        "SELECT f.id, f.upstream_id, COALESCE(u.name, 'Unknown') AS upstream_name,
                f.action, f.reason, f.timestamp
         FROM upstream_failover_log f
         LEFT JOIN dns_upstreams u ON u.id = f.upstream_id
         ORDER BY f.timestamp DESC
         LIMIT 100",
    )
    .fetch_all(&state.db)
    .await?;

    let data: Vec<Value> = rows
        .into_iter()
        .map(
            |(id, upstream_id, upstream_name, action, reason, timestamp)| {
                json!({
                    "id": id,
                    "upstream_id": upstream_id,
                    "upstream_name": upstream_name,
                    "action": action,
                    "reason": reason,
                    "timestamp": timestamp,
                })
            },
        )
        .collect();

    Ok(Json(json!({ "data": data, "total": data.len() })))
}

/// Simple DNS connectivity test using hickory-resolver.
///
/// Supports:
/// - Plain UDP: `"1.1.1.1"` or `"1.1.1.1:53"`
/// - DoH: `"https://dns.cloudflare.com/dns-query"` or `"https://1.1.1.1/dns-query"`
/// - DoT: `"tls://1.1.1.1"` or `"tls://dns.google:853"`
async fn test_dns_connectivity(addr: &str, timeout: std::time::Duration) -> anyhow::Result<()> {
    use hickory_resolver::config::{
        NameServerConfig, NameServerConfigGroup, Protocol, ResolverConfig,
    };
    use hickory_resolver::TokioAsyncResolver;
    use std::net::ToSocketAddrs;

    let config = if addr.starts_with("https://") {
        // DoH connectivity test
        let (host, port, _path) = parse_doh_host_port(addr)?;
        let lookup_target = format!("{}:{}", host, port);
        let addrs: Vec<std::net::IpAddr> = lookup_target
            .as_str()
            .to_socket_addrs()
            .map_err(|e| anyhow::anyhow!("Failed to resolve DoH host '{}': {}", host, e))?
            .map(|a| a.ip())
            .collect();

        if addrs.is_empty() {
            anyhow::bail!("DoH host '{}' resolved to no addresses", host);
        }

        let ns_group = NameServerConfigGroup::from_ips_https(&addrs, port, host, false);
        let mut cfg = ResolverConfig::new();
        for ns in ns_group.into_inner() {
            cfg.add_name_server(ns);
        }
        cfg
    } else if addr.starts_with("tls://") {
        // DoT connectivity test — performs a real TLS handshake via hickory
        let rest = addr.strip_prefix("tls://").unwrap();
        let authority = match rest.find('/') {
            Some(idx) => &rest[..idx],
            None => rest,
        };
        let (host, port) = match authority.rfind(':') {
            Some(idx) => (
                authority[..idx].to_string(),
                authority[idx + 1..].parse::<u16>()?,
            ),
            None => (authority.to_string(), 853u16),
        };
        let lookup_target = format!("{}:{}", host, port);
        let addrs: Vec<std::net::IpAddr> = lookup_target
            .as_str()
            .to_socket_addrs()
            .map_err(|e| anyhow::anyhow!("Failed to resolve DoT host '{}': {}", host, e))?
            .map(|a| a.ip())
            .collect();

        if addrs.is_empty() {
            anyhow::bail!("DoT host '{}' resolved to no addresses", host);
        }

        let ns_group = NameServerConfigGroup::from_ips_tls(&addrs, port, host, false);
        let mut cfg = ResolverConfig::new();
        for ns in ns_group.into_inner() {
            cfg.add_name_server(ns);
        }
        cfg
    } else {
        // Plain UDP connectivity test
        let (ip, port) = if addr.contains(':') {
            let parts: Vec<&str> = addr.rsplitn(2, ':').collect();
            (parts[1], parts[0].parse::<u16>()?)
        } else {
            (addr, 53)
        };

        let ip_addr = ip.parse::<std::net::IpAddr>()?;
        let mut cfg = ResolverConfig::new();
        cfg.add_name_server(NameServerConfig {
            socket_addr: (ip_addr, port).into(),
            protocol: Protocol::Udp,
            trust_negative_responses: false,
            tls_config: None,
            bind_addr: None,
            tls_dns_name: None,
        });
        cfg
    };

    let opts = hickory_resolver::config::ResolverOpts::default();
    let resolver = TokioAsyncResolver::tokio(config, opts);

    // Try a simple lookup with timeout
    let _ = tokio::time::timeout(timeout, resolver.lookup_ip("example.com.")).await??;

    Ok(())
}

/// Extract (host, port) from a DoH URL like `https://hostname[:port][/path]`.
fn parse_doh_host_port(url: &str) -> anyhow::Result<(String, u16, String)> {
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| anyhow::anyhow!("URL must start with https://: {}", url))?;

    let (authority, path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], rest[idx..].to_string()),
        None => (rest, "/dns-query".to_string()),
    };

    let (host, port) = if authority.starts_with('[') {
        // IPv6 literal: [::1] or [::1]:443
        let end_bracket = authority
            .rfind(']')
            .ok_or_else(|| anyhow::anyhow!("Malformed IPv6 address in URL: {}", url))?;
        let host_part = authority[1..end_bracket].to_string();
        let port_part = &authority[end_bracket + 1..];
        let port = if port_part.is_empty() {
            443u16
        } else {
            port_part
                .strip_prefix(':')
                .and_then(|p| p.parse::<u16>().ok())
                .ok_or_else(|| anyhow::anyhow!("Invalid port in URL: {}", url))?
        };
        (host_part, port)
    } else {
        match authority.rfind(':') {
            Some(idx) => {
                let host_part = authority[..idx].to_string();
                let port_str = &authority[idx + 1..];
                let port = port_str
                    .parse::<u16>()
                    .map_err(|_| anyhow::anyhow!("Invalid port '{}' in URL: {}", port_str, url))?;
                (host_part, port)
            }
            None => (authority.to_string(), 443u16),
        }
    };

    if host.is_empty() {
        anyhow::bail!("Empty host in DoH URL: {}", url);
    }

    Ok((host, port, path))
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
