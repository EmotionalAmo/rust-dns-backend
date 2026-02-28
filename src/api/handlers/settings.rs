use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::api::middleware::rbac::AdminUser;
use crate::api::AppState;
use crate::error::{AppError, AppResult};

#[derive(Debug, Deserialize)]
pub struct UpdateDnsSettingsRequest {
    pub upstreams: Option<Vec<String>>,
    pub cache_ttl: Option<u64>,
    pub query_log_retention_days: Option<u64>,
    pub stats_retention_days: Option<u64>,
    pub safe_search_enabled: Option<bool>,
    pub parental_control_enabled: Option<bool>,
    pub parental_control_level: Option<String>,
}

/// Get current DNS settings
pub async fn get_dns(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> AppResult<Json<Value>> {
    // Fetch settings from database
    let cache_ttl: (String,) =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'dns_cache_ttl'")
            .fetch_one(&state.db)
            .await
            .unwrap_or(("300".to_string(),));

    let query_log_retention: (String,) =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'query_log_retention_days'")
            .fetch_one(&state.db)
            .await
            .unwrap_or(("30".to_string(),));

    let stats_retention: (String,) =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'stats_retention_days'")
            .fetch_one(&state.db)
            .await
            .unwrap_or(("90".to_string(),));

    let safe_search: (String,) =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'safe_search_enabled'")
            .fetch_one(&state.db)
            .await
            .unwrap_or(("false".to_string(),));

    let parental_control: (String,) =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'parental_control_enabled'")
            .fetch_one(&state.db)
            .await
            .unwrap_or(("false".to_string(),));

    let parental_level: (String,) =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'parental_control_level'")
            .fetch_one(&state.db)
            .await
            .unwrap_or(("none".to_string(),));

    // Parse values
    let cache_ttl = cache_ttl.0.parse::<u64>().unwrap_or(300);
    let query_log_retention = query_log_retention.0.parse::<u64>().unwrap_or(30);
    let stats_retention = stats_retention.0.parse::<u64>().unwrap_or(90);
    let safe_search_enabled = safe_search.0 == "true";
    let parental_control_enabled = parental_control.0 == "true";
    let parental_control_level = parental_level.0;

    // Get upstreams from config (or database if implemented)
    // For now, return empty array as default
    let upstreams: Vec<String> = vec![
        "https://1.1.1.1/dns-query".to_string(), // Cloudflare
        "https://8.8.8.8/dns-query".to_string(), // Google
    ];

    Ok(Json(json!({
        "upstreams": upstreams,
        "cache_ttl": cache_ttl,
        "query_log_retention_days": query_log_retention,
        "stats_retention_days": stats_retention,
        "safe_search_enabled": safe_search_enabled,
        "parental_control_enabled": parental_control_enabled,
        "parental_control_level": parental_control_level,
    })))
}

/// Update DNS settings
pub async fn update_dns(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(body): Json<UpdateDnsSettingsRequest>,
) -> AppResult<Json<Value>> {
    // Update cache_ttl if provided
    if let Some(cache_ttl) = body.cache_ttl {
        if cache_ttl > 86400 {
            return Err(AppError::Validation(
                "cache_ttl must be between 0 and 86400 seconds".to_string(),
            ));
        }
        sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('dns_cache_ttl', ?)")
            .bind(cache_ttl.to_string())
            .execute(&state.db)
            .await?;
    }

    // Update query_log_retention_days if provided
    if let Some(days) = body.query_log_retention_days {
        if days == 0 || days > 365 {
            return Err(AppError::Validation(
                "query_log_retention_days must be between 1 and 365".to_string(),
            ));
        }
        sqlx::query(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('query_log_retention_days', ?)",
        )
        .bind(days.to_string())
        .execute(&state.db)
        .await?;
    }

    // Update stats_retention_days if provided
    if let Some(days) = body.stats_retention_days {
        if days == 0 || days > 365 {
            return Err(AppError::Validation(
                "stats_retention_days must be between 1 and 365".to_string(),
            ));
        }
        sqlx::query(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('stats_retention_days', ?)",
        )
        .bind(days.to_string())
        .execute(&state.db)
        .await?;
    }

    // Update safe_search_enabled if provided
    if let Some(enabled) = body.safe_search_enabled {
        sqlx::query(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('safe_search_enabled', ?)",
        )
        .bind(if enabled { "true" } else { "false" })
        .execute(&state.db)
        .await?;
        // Reload filter engine to apply/remove Safe Search rewrites
        if let Err(e) = state.filter.reload().await {
            tracing::error!(
                "Failed to reload filter engine after Safe Search update: {}",
                e
            );
        }
    }

    // Update parental_control_enabled if provided
    if let Some(enabled) = body.parental_control_enabled {
        sqlx::query(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('parental_control_enabled', ?)",
        )
        .bind(if enabled { "true" } else { "false" })
        .execute(&state.db)
        .await?;
        // Reload filter engine when Parental Control changes
        if let Err(e) = state.filter.reload().await {
            tracing::error!(
                "Failed to reload filter engine after Parental Control update: {}",
                e
            );
        }
    }

    // Update parental_control_level if provided
    if let Some(level) = body.parental_control_level {
        // Validate level value
        if !matches!(level.as_str(), "none" | "basic" | "standard" | "strict") {
            return Err(AppError::Validation(
                "parental_control_level must be one of: none, basic, standard, strict".to_string(),
            ));
        }
        sqlx::query(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('parental_control_level', ?)",
        )
        .bind(&level)
        .execute(&state.db)
        .await?;
        // Reload filter engine to apply/remove Parental Control rules
        if let Err(e) = state.filter.reload().await {
            tracing::error!(
                "Failed to reload filter engine after Parental Control level update: {}",
                e
            );
        }
    }

    // Note: Upstreams would require either a settings table update or config file reload
    // For this implementation, we acknowledge update but don't persist upstreams
    if body.upstreams.is_some() {
        tracing::warn!("upstreams update requested but not implemented (requires config reload)");
    }

    Ok(Json(json!({"success": true})))
}
