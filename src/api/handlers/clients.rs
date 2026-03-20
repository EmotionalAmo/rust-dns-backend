use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

use crate::api::middleware::auth::AuthUser;
use crate::api::middleware::client_ip::ClientIp;
use crate::api::AppState;
use crate::error::{AppError, AppResult};

#[derive(Debug, Deserialize)]
pub struct CreateClientRequest {
    pub name: String,
    pub identifiers: serde_json::Value,
    pub upstreams: Option<serde_json::Value>,
    #[serde(default = "default_filter_enabled")]
    pub filter_enabled: bool,
    pub tags: Option<serde_json::Value>,
}

fn default_filter_enabled() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct UpdateClientRequest {
    pub name: Option<String>,
    pub identifiers: Option<serde_json::Value>,
    pub upstreams: Option<serde_json::Value>,
    pub filter_enabled: Option<bool>,
    pub tags: Option<serde_json::Value>,
}

fn validate_json_array(value: &serde_json::Value) -> AppResult<()> {
    if let Some(arr) = value.as_array() {
        if arr.is_empty() {
            return Err(AppError::Validation("Array cannot be empty".to_string()));
        }
        Ok(())
    } else {
        Err(AppError::Validation("Must be a JSON array".to_string()))
    }
}

fn parse_json_value(value: &Option<String>) -> Option<Value> {
    value.as_ref().and_then(|s| serde_json::from_str(s).ok())
}

type ClientRow = (
    String,
    String,
    String,
    Option<String>,
    bool,
    Option<String>,
    String,
    String,
    i64,
);

pub async fn list(State(state): State<Arc<AppState>>, _auth: AuthUser) -> AppResult<Json<Value>> {
    // 1. Fetch static clients
    let static_rows: Vec<ClientRow> = sqlx::query_as(
        "SELECT c.id, c.name, c.identifiers, c.upstreams, c.filter_enabled, c.tags, c.created_at, c.updated_at,
                COALESCE(
                  (SELECT COUNT(*) FROM query_log ql
                   WHERE ql.client_ip IN (SELECT value FROM json_array_elements_text(c.identifiers::json))),
                  0
                ) AS query_count
         FROM clients c ORDER BY c.created_at DESC",
    )
    .fetch_all(&state.db)
    .await?;

    // 2. Fetch dynamic clients (IPs from query log that are not in any static client's identifiers)
    let dynamic_ips: Vec<(String, i64, String)> = sqlx::query_as(
        r#"
        SELECT
            ql.client_ip,
            COUNT(*) as query_count,
            MAX(ql.time)::TEXT as last_seen
        FROM query_log ql
        WHERE NOT EXISTS (
            SELECT 1 FROM clients c
            WHERE ql.client_ip IN (SELECT value FROM json_array_elements_text(c.identifiers::json))
        )
        GROUP BY ql.client_ip
        ORDER BY last_seen DESC
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    // 3. Get ARP map
    let arp_map = crate::utils::arp::get_arp_map();
    let has_mac_regex = regex::Regex::new(r"^([0-9a-fA-F]{2}[:-]){5}[0-9a-fA-F]{2}$").unwrap();

    let mut data: Vec<Value> = Vec::new();

    // Process static clients
    for (
        id,
        name,
        identifiers,
        upstreams,
        filter_enabled,
        tags,
        created_at,
        updated_at,
        query_count,
    ) in static_rows
    {
        let mut idents: Vec<String> = serde_json::from_str(&identifiers).unwrap_or_default();

        // Auto-inject MAC from ARP if any IP doesn't have a matching MAC
        // Check if we already have a MAC address manually defined
        let has_mac = idents.iter().any(|i| has_mac_regex.is_match(i));

        if !has_mac {
            // Find IP and look up in ARP
            if let Some(ip) = idents.iter().find(|i| !has_mac_regex.is_match(i)) {
                if let Some(mac) = arp_map.get(ip) {
                    idents.push(mac.clone());
                }
            }
        }

        data.push(json!({
            "id": id,
            "name": name,
            "identifiers": idents,
            "upstreams": parse_json_value(&upstreams),
            "filter_enabled": filter_enabled,
            "tags": parse_json_value(&tags),
            "created_at": created_at,
            "updated_at": updated_at,
            "query_count": query_count,
            "is_static": true,
        }));
    }

    // Process dynamic clients
    for (ip, query_count, last_seen) in dynamic_ips {
        let mut idents = vec![ip.clone()];
        if let Some(mac) = arp_map.get(&ip) {
            idents.push(mac.clone());
        }

        data.push(json!({
            "id": format!("dynamic-{}", ip),
            "name": format!("Unknown {}", ip), // Or attempt to resolve hostname if needed
            "identifiers": idents,
            "upstreams": serde_json::Value::Null,
            "filter_enabled": true, // Default to true so global filters apply
            "tags": serde_json::Value::Null,
            "created_at": last_seen.clone(),
            "updated_at": last_seen,
            "query_count": query_count,
            "is_static": false,
        }));
    }

    // Sort all by updated_at / last_seen descending
    data.sort_by(|a, b| {
        let date_a = a.get("updated_at").and_then(|v| v.as_str()).unwrap_or("");
        let date_b = b.get("updated_at").and_then(|v| v.as_str()).unwrap_or("");
        date_b.cmp(date_a)
    });

    let count = data.len();
    Ok(Json(json!({ "data": data, "total": count })))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    ClientIp(ip): ClientIp,
    auth: AuthUser,
    Json(body): Json<CreateClientRequest>,
) -> AppResult<Json<Value>> {
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::Validation(
            "Client name cannot be empty".to_string(),
        ));
    }

    // Validate identifiers is a non-empty array
    validate_json_array(&body.identifiers)?;

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let identifiers_str = serde_json::to_string(&body.identifiers)
        .map_err(|e| AppError::Internal(format!("Failed to serialize identifiers: {}", e)))?;
    let upstreams_str = body
        .upstreams
        .as_ref()
        .map(|v| {
            serde_json::to_string(v)
                .map_err(|e| AppError::Internal(format!("Failed to serialize upstreams: {}", e)))
        })
        .transpose()?;
    let tags_str = body
        .tags
        .as_ref()
        .map(|v| {
            serde_json::to_string(v)
                .map_err(|e| AppError::Internal(format!("Failed to serialize tags: {}", e)))
        })
        .transpose()?;
    let filter_enabled = body.filter_enabled;

    sqlx::query(
        "INSERT INTO clients (id, name, identifiers, upstreams, filter_enabled, tags, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"
    )
    .bind(&id)
    .bind(&name)
    .bind(&identifiers_str)
    .bind(&upstreams_str)
    .bind(filter_enabled)
    .bind(&tags_str)
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await?;

    state.dns_handler.invalidate_all_client_cache().await;

    crate::db::audit::log_action(
        state.db.clone(),
        auth.0.sub.clone(),
        auth.0.username.clone(),
        "create",
        "client",
        Some(id.clone()),
        Some(name.clone()),
        ip,
    );

    Ok(Json(json!({
        "id": id,
        "name": name,
        "identifiers": body.identifiers,
        "upstreams": body.upstreams,
        "filter_enabled": body.filter_enabled,
        "tags": body.tags,
        "created_at": now,
        "updated_at": now,
    })))
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    ClientIp(ip): ClientIp,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<UpdateClientRequest>,
) -> AppResult<Json<Value>> {
    // Check if client exists
    let existing: Option<ClientRow> = sqlx::query_as(
        "SELECT c.id, c.name, c.identifiers, c.upstreams, c.filter_enabled, c.tags, c.created_at, c.updated_at,
                COALESCE(
                  (SELECT COUNT(*) FROM query_log ql
                   WHERE ql.client_ip IN (SELECT value FROM json_array_elements_text(c.identifiers::json))),
                  0
                ) AS query_count
         FROM clients c WHERE c.id = $1",
    )
    .bind(&id)
    .fetch_optional(&state.db)
    .await?;

    let (
        _,
        old_name,
        old_identifiers,
        old_upstreams,
        old_filter_enabled,
        old_tags,
        created_at,
        _updated_at,
        _query_count,
    ) = existing.ok_or_else(|| AppError::NotFound(format!("Client {} not found", id)))?;

    // Prepare new values
    let name = body.name.unwrap_or(old_name);

    // Handle identifiers
    let identifiers = if let Some(ref new_identifiers) = body.identifiers {
        validate_json_array(new_identifiers)?;
        serde_json::to_string(new_identifiers)
            .map_err(|e| AppError::Internal(format!("Failed to serialize identifiers: {}", e)))?
    } else {
        old_identifiers
    };

    // Handle upstreams
    let upstreams = if let Some(ref new_upstreams) = body.upstreams {
        Some(
            serde_json::to_string(new_upstreams)
                .map_err(|e| AppError::Internal(format!("Failed to serialize upstreams: {}", e)))?,
        )
    } else {
        old_upstreams
    };

    // Handle tags
    let tags = if let Some(ref new_tags) = body.tags {
        Some(
            serde_json::to_string(new_tags)
                .map_err(|e| AppError::Internal(format!("Failed to serialize tags: {}", e)))?,
        )
    } else {
        old_tags
    };

    let filter_enabled = body.filter_enabled.unwrap_or(old_filter_enabled);

    let now = Utc::now().to_rfc3339();

    sqlx::query(
        "UPDATE clients SET name = $1, identifiers = $2, upstreams = $3, filter_enabled = $4, tags = $5, updated_at = $6
         WHERE id = $7"
    )
    .bind(&name)
    .bind(&identifiers)
    .bind(&upstreams)
    .bind(filter_enabled)
    .bind(&tags)
    .bind(&now)
    .bind(&id)
    .execute(&state.db)
    .await?;

    state.dns_handler.invalidate_all_client_cache().await;

    // Parse for response
    let identifiers_json = parse_json_value(&Some(identifiers));
    let tags_json = parse_json_value(&tags);
    let upstreams_json = parse_json_value(&upstreams);

    crate::db::audit::log_action(
        state.db.clone(),
        auth.0.sub.clone(),
        auth.0.username.clone(),
        "update",
        "client",
        Some(id.clone()),
        Some(name.clone()),
        ip,
    );

    Ok(Json(json!({
        "id": id,
        "name": name,
        "identifiers": identifiers_json,
        "upstreams": upstreams_json,
        "filter_enabled": filter_enabled,
        "tags": tags_json,
        "created_at": created_at,
        "updated_at": now,
    })))
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    ClientIp(ip): ClientIp,
    auth: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    let result = sqlx::query("DELETE FROM clients WHERE id = $1")
        .bind(&id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("Client {} not found", id)));
    }

    state.dns_handler.invalidate_all_client_cache().await;

    crate::db::audit::log_action(
        state.db.clone(),
        auth.0.sub.clone(),
        auth.0.username.clone(),
        "delete",
        "client",
        Some(id.clone()),
        None,
        ip,
    );

    Ok(Json(json!({"success": true})))
}

#[derive(Debug, Deserialize)]
pub struct ActivityQuery {
    pub hours: Option<i64>,
}

pub async fn get_activity(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(id): Path<String>,
    Query(params): Query<ActivityQuery>,
) -> AppResult<Json<Value>> {
    let hours = params.hours.unwrap_or(24).clamp(1, 168);
    let time_filter = format!("-{} hours", hours);
    let mac_re = regex::Regex::new(r"^([0-9a-fA-F]{2}[:-]){5}[0-9a-fA-F]{2}$").unwrap();

    // Determine IPs for this client
    let ips: Vec<String> = if let Some(ip) = id.strip_prefix("dynamic-") {
        vec![ip.to_string()]
    } else {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT identifiers FROM clients WHERE id = $1")
                .bind(&id)
                .fetch_optional(&state.db)
                .await?;

        match row {
            None => return Err(AppError::NotFound(format!("Client {} not found", id))),
            Some((identifiers,)) => {
                let idents: Vec<String> = serde_json::from_str(&identifiers).unwrap_or_default();
                idents.into_iter().filter(|i| !mac_re.is_match(i)).collect()
            }
        }
    };

    if ips.is_empty() {
        return Ok(Json(json!({ "data": [], "top_domains": [] })));
    }

    let ips_json = serde_json::to_string(&ips).unwrap_or_else(|_| "[]".to_string());

    // Hourly activity buckets
    let activity_rows: Vec<(String, i64, i64)> = sqlx::query_as(
        r#"
        SELECT
            TO_CHAR(time, 'YYYY-MM-DD"T"HH24:00:00') as hour,
            COUNT(*) as total,
            SUM(CASE WHEN status = 'blocked' THEN 1 ELSE 0 END) as blocked
        FROM query_log
        WHERE client_ip IN (SELECT value FROM jsonb_array_elements_text($1::jsonb))
          AND time >= NOW() + $2::interval
        GROUP BY hour
        ORDER BY hour ASC
        "#,
    )
    .bind(&ips_json)
    .bind(&time_filter)
    .fetch_all(&state.db)
    .await?;

    // Top queried domains
    let top_domains: Vec<(String, i64)> = sqlx::query_as(
        r#"
        SELECT question, COUNT(*) as count
        FROM query_log
        WHERE client_ip IN (SELECT value FROM jsonb_array_elements_text($1::jsonb))
          AND time >= NOW() + $2::interval
        GROUP BY question
        ORDER BY count DESC
        LIMIT 10
        "#,
    )
    .bind(&ips_json)
    .bind(&time_filter)
    .fetch_all(&state.db)
    .await?;

    let data: Vec<Value> = activity_rows
        .into_iter()
        .map(|(hour, total, blocked)| json!({ "hour": hour, "total": total, "blocked": blocked }))
        .collect();

    let top_domains_json: Vec<Value> = top_domains
        .into_iter()
        .map(|(domain, count)| json!({ "domain": domain, "count": count }))
        .collect();

    Ok(Json(
        json!({ "data": data, "top_domains": top_domains_json }),
    ))
}

#[derive(Debug, Deserialize)]
pub struct PtrQuery {
    pub ip: String,
}

pub async fn ptr_lookup(
    State(_state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(params): Query<PtrQuery>,
) -> AppResult<Json<Value>> {
    let ip: std::net::IpAddr = params
        .ip
        .parse()
        .map_err(|_| AppError::Validation(format!("Invalid IP address: {}", params.ip)))?;

    // Use system resolver so PTR queries route through the local network (router)
    let (cfg, mut opts) = hickory_resolver::system_conf::read_system_conf().unwrap_or_else(|_| {
        (
            hickory_resolver::config::ResolverConfig::cloudflare(),
            hickory_resolver::config::ResolverOpts::default(),
        )
    });
    opts.timeout = std::time::Duration::from_secs(3);
    opts.attempts = 1;

    let resolver = hickory_resolver::TokioAsyncResolver::tokio(cfg, opts);

    match tokio::time::timeout(
        std::time::Duration::from_secs(4),
        resolver.reverse_lookup(ip),
    )
    .await
    {
        Ok(Ok(lookup)) => {
            let name = lookup
                .iter()
                .next()
                .map(|n| n.to_string().trim_end_matches('.').to_string());
            Ok(Json(serde_json::json!({ "ptr": name })))
        }
        _ => Ok(Json(serde_json::json!({ "ptr": null }))),
    }
}
