use axum::{
    extract::{Path, State},
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
    i64,
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
                   WHERE ql.client_ip IN (SELECT value FROM json_each(c.identifiers))),
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
            MAX(ql.time) as last_seen
        FROM query_log ql
        WHERE NOT EXISTS (
            SELECT 1 FROM clients c, json_each(c.identifiers) j
            WHERE j.value = ql.client_ip
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
            "filter_enabled": filter_enabled == 1,
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
    let filter_enabled = if body.filter_enabled { 1 } else { 0 };

    sqlx::query(
        "INSERT INTO clients (id, name, identifiers, upstreams, filter_enabled, tags, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
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
                   WHERE ql.client_ip IN (SELECT value FROM json_each(c.identifiers))),
                  0
                ) AS query_count
         FROM clients c WHERE c.id = ?",
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

    let filter_enabled = body
        .filter_enabled
        .map(|b| if b { 1 } else { 0 })
        .unwrap_or(old_filter_enabled);

    let now = Utc::now().to_rfc3339();

    sqlx::query(
        "UPDATE clients SET name = ?, identifiers = ?, upstreams = ?, filter_enabled = ?, tags = ?, updated_at = ?
         WHERE id = ?"
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
        "filter_enabled": filter_enabled == 1,
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
    let result = sqlx::query("DELETE FROM clients WHERE id = ?")
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
