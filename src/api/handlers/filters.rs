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
pub struct CreateFilterRequest {
    pub name: String,
    pub url: Option<String>,
    #[serde(default = "default_enabled")]
    pub is_enabled: bool,
    #[serde(default)]
    pub update_interval_hours: i64,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct UpdateFilterRequest {
    pub name: Option<String>,
    pub url: Option<String>,
    pub is_enabled: Option<bool>,
    pub update_interval_hours: Option<i64>,
}

type FilterRow = (
    String,
    String,
    Option<String>,
    i64,
    i64,
    Option<String>,
    String,
    i64,
);

pub async fn list(State(state): State<Arc<AppState>>, _auth: AuthUser) -> AppResult<Json<Value>> {
    let rows: Vec<FilterRow> = sqlx::query_as(
        "SELECT id, name, url, is_enabled, rule_count, last_updated, created_at, update_interval_hours
         FROM filter_lists ORDER BY created_at DESC",
    )
    .fetch_all(&state.db)
    .await?;

    let data: Vec<Value> = rows
        .into_iter()
        .map(
            |(
                id,
                name,
                url,
                is_enabled,
                rule_count,
                last_updated,
                created_at,
                update_interval_hours,
            )| {
                json!({
                    "id": id,
                    "name": name,
                    "url": url,
                    "is_enabled": is_enabled == 1,
                    "rule_count": rule_count,
                    "last_updated": last_updated,
                    "created_at": created_at,
                    "update_interval_hours": update_interval_hours,
                })
            },
        )
        .collect();
    let count = data.len();
    Ok(Json(json!({ "data": data, "total": count })))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    ClientIp(ip): ClientIp,
    auth: AuthUser,
    Json(body): Json<CreateFilterRequest>,
) -> AppResult<Json<Value>> {
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::Validation(
            "Filter name cannot be empty".to_string(),
        ));
    }

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let is_enabled = if body.is_enabled { 1 } else { 0 };

    sqlx::query(
        "INSERT INTO filter_lists (id, name, url, is_enabled, rule_count, last_updated, created_at, update_interval_hours)
         VALUES ($1, $2, $3, $4, 0, NULL, $5, $6)",
    )
    .bind(&id)
    .bind(&name)
    .bind(&body.url)
    .bind(is_enabled)
    .bind(&now)
    .bind(body.update_interval_hours)
    .execute(&state.db)
    .await
    .map_err(|e| {
        // PostgreSQL unique violation (code 23505) — duplicate URL
        if let sqlx::Error::Database(ref db_err) = e {
            if db_err.code().as_deref() == Some("23505") {
                return AppError::Conflict(
                    "A filter list with this URL already exists".to_string(),
                );
            }
        }
        AppError::Internal(e.to_string())
    })?;

    // If URL provided, spawn background sync so the HTTP response returns immediately.
    // Large filter lists (AdGuard, 50k+ rules) can take minutes to fetch—do NOT block here.
    let syncing = if let Some(ref url) = body.url {
        let db = state.db.clone();
        let filter_engine = state.filter.clone();
        let filter_id = id.clone();
        let url = url.clone();
        tokio::spawn(async move {
            match crate::dns::subscription::sync_filter_list(&db, &filter_id, &url).await {
                Ok(n) => {
                    tracing::info!("Background sync filter {}: {} rules", filter_id, n);
                    let _ = filter_engine.reload().await;
                }
                Err(e) => tracing::warn!("Background sync filter {}: {}", filter_id, e),
            }
        });
        true
    } else {
        false
    };

    crate::db::audit::log_action(
        state.db.clone(),
        auth.0.sub.clone(),
        auth.0.username.clone(),
        "create",
        "filter",
        Some(id.clone()),
        Some(name.clone()),
        ip.clone(),
    );

    Ok(Json(json!({
        "id": id,
        "name": name,
        "url": body.url,
        "is_enabled": body.is_enabled,
        "rule_count": 0,
        "last_updated": null,
        "created_at": now,
        "syncing": syncing,
        "update_interval_hours": body.update_interval_hours,
    })))
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    ClientIp(ip): ClientIp,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<UpdateFilterRequest>,
) -> AppResult<Json<Value>> {
    // Check if filter exists
    let existing: Option<FilterRow> = sqlx::query_as(
        "SELECT id, name, url, is_enabled, rule_count, last_updated, created_at, update_interval_hours
         FROM filter_lists WHERE id = $1",
    )
    .bind(&id)
    .fetch_optional(&state.db)
    .await?;

    let (
        _,
        old_name,
        old_url,
        old_enabled,
        old_rule_count,
        old_last_updated,
        created_at,
        old_interval,
    ) = existing.ok_or_else(|| AppError::NotFound(format!("Filter list {} not found", id)))?;

    let name = body.name.unwrap_or(old_name);
    let url = body.url.or(old_url);
    let is_enabled = body
        .is_enabled
        .map(|b| if b { 1 } else { 0 })
        .unwrap_or(old_enabled);
    let update_interval_hours = body.update_interval_hours.unwrap_or(old_interval);

    sqlx::query("UPDATE filter_lists SET name = $1, url = $2, is_enabled = $3, update_interval_hours = $4 WHERE id = $5")
        .bind(&name)
        .bind(&url)
        .bind(is_enabled)
        .bind(update_interval_hours)
        .bind(&id)
        .execute(&state.db)
        .await?;

    // Hot-reload filter engine
    state
        .filter
        .reload()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    crate::db::audit::log_action(
        state.db.clone(),
        auth.0.sub.clone(),
        auth.0.username.clone(),
        "update",
        "filter",
        Some(id.clone()),
        Some(name.clone()),
        ip.clone(),
    );

    Ok(Json(json!({
        "id": id,
        "name": name,
        "url": url,
        "is_enabled": is_enabled == 1,
        "rule_count": old_rule_count,
        "last_updated": old_last_updated,
        "created_at": created_at,
        "update_interval_hours": update_interval_hours,
    })))
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    ClientIp(ip): ClientIp,
    auth: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    // Delete associated rules first
    sqlx::query("DELETE FROM custom_rules WHERE created_by = $1")
        .bind(format!("filter:{}", id))
        .execute(&state.db)
        .await?;

    let result = sqlx::query("DELETE FROM filter_lists WHERE id = $1")
        .bind(&id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("Filter list {} not found", id)));
    }

    // Hot-reload filter engine
    state
        .filter
        .reload()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    crate::db::audit::log_action(
        state.db.clone(),
        auth.0.sub.clone(),
        auth.0.username.clone(),
        "delete",
        "filter",
        Some(id.clone()),
        None,
        ip.clone(),
    );

    Ok(Json(json!({"success": true})))
}

/// Manually refresh a remote filter list
pub async fn refresh(
    State(state): State<Arc<AppState>>,
    ClientIp(ip): ClientIp,
    auth: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    // Get filter list info
    let filter: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT name, url FROM filter_lists WHERE id = $1")
            .bind(&id)
            .fetch_optional(&state.db)
            .await?;

    let (name, url) =
        filter.ok_or_else(|| AppError::NotFound(format!("Filter list {} not found", id)))?;

    let url = url.ok_or_else(|| {
        AppError::Validation("Cannot refresh local filter list (no URL configured)".to_string())
    })?;

    // Spawn background sync — large remote lists can take >30s to fetch+insert.
    // Return immediately so the browser doesn't time out.
    let db = state.db.clone();
    let filter_engine = state.filter.clone();
    let filter_id = id.clone();
    tokio::spawn(async move {
        match crate::dns::subscription::sync_filter_list(&db, &filter_id, &url).await {
            Ok(n) => {
                tracing::info!("Background refresh filter {}: {} rules", filter_id, n);
                let _ = filter_engine.reload().await;
            }
            Err(e) => tracing::warn!("Background refresh filter {}: {}", filter_id, e),
        }
    });

    crate::db::audit::log_action(
        state.db.clone(),
        auth.0.sub.clone(),
        auth.0.username.clone(),
        "refresh",
        "filter",
        Some(id.clone()),
        Some(name.clone()),
        ip.clone(),
    );

    Ok(Json(json!({
        "id": id,
        "name": name,
        "success": true,
        "syncing": true,
        "message": "同步已在后台启动，请稍后刷新查看结果"
    })))
}
