use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::api::middleware::client_ip::ClientIp;
use crate::api::middleware::rbac::AdminUser;
use crate::api::AppState;
use crate::dns::acl::Acl;
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
    pub upstream_strategy: Option<String>,
    /// CIDR list of allowed client networks (e.g. ["192.168.1.0/24", "10.0.0.0/8"])
    pub acl_allowed_networks: Option<Vec<String>>,
    /// CIDR list of denied client networks
    pub acl_denied_networks: Option<Vec<String>>,
}

/// Get current DNS settings
pub async fn get_dns(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> AppResult<Json<Value>> {
    // Fetch all settings in a single query
    let keys = vec![
        "dns_cache_ttl",
        "query_log_retention_days",
        "stats_retention_days",
        "safe_search_enabled",
        "parental_control_enabled",
        "parental_control_level",
        "upstream_strategy",
        "acl_allowed_networks",
        "acl_denied_networks",
    ];
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT key, value FROM settings WHERE key = ANY($1)")
            .bind(&keys)
            .fetch_all(&state.db)
            .await
            .unwrap_or_default();
    let map: std::collections::HashMap<String, String> = rows.into_iter().collect();
    let get = |k: &str, default: &str| map.get(k).cloned().unwrap_or_else(|| default.to_string());

    // Parse values
    let cache_ttl = get("dns_cache_ttl", "300").parse::<u64>().unwrap_or(300);
    let query_log_retention = get("query_log_retention_days", "30")
        .parse::<u64>()
        .unwrap_or(30);
    let stats_retention = get("stats_retention_days", "90")
        .parse::<u64>()
        .unwrap_or(90);
    let safe_search_enabled = get("safe_search_enabled", "false") == "true";
    let parental_control_enabled = get("parental_control_enabled", "false") == "true";
    let parental_control_level = get("parental_control_level", "none");
    let upstream_strategy = get("upstream_strategy", "priority");

    // Upstreams are managed via /api/v1/settings/upstreams
    let upstreams: Vec<String> = vec![];

    let acl_allowed_networks: Vec<String> =
        serde_json::from_str(&get("acl_allowed_networks", "[]")).unwrap_or_default();
    let acl_denied_networks: Vec<String> =
        serde_json::from_str(&get("acl_denied_networks", "[]")).unwrap_or_default();

    Ok(Json(json!({
        "upstreams": upstreams,
        "cache_ttl": cache_ttl,
        "query_log_retention_days": query_log_retention,
        "stats_retention_days": stats_retention,
        "safe_search_enabled": safe_search_enabled,
        "parental_control_enabled": parental_control_enabled,
        "parental_control_level": parental_control_level,
        "upstream_strategy": upstream_strategy,
        "acl_allowed_networks": acl_allowed_networks,
        "acl_denied_networks": acl_denied_networks,
    })))
}

/// Update DNS settings
pub async fn update_dns(
    State(state): State<Arc<AppState>>,
    ClientIp(ip): ClientIp,
    admin: AdminUser,
    Json(body): Json<UpdateDnsSettingsRequest>,
) -> AppResult<Json<Value>> {
    // Update cache_ttl if provided
    if let Some(cache_ttl) = body.cache_ttl {
        if cache_ttl > 86400 {
            return Err(AppError::Validation(
                "cache_ttl must be between 0 and 86400 seconds".to_string(),
            ));
        }
        sqlx::query("INSERT INTO settings (key, value) VALUES ('dns_cache_ttl', $1) ON CONFLICT (key) DO UPDATE SET value = $1")
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
            "INSERT INTO settings (key, value) VALUES ('query_log_retention_days', $1) ON CONFLICT (key) DO UPDATE SET value = $1",
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
            "INSERT INTO settings (key, value) VALUES ('stats_retention_days', $1) ON CONFLICT (key) DO UPDATE SET value = $1",
        )
        .bind(days.to_string())
        .execute(&state.db)
        .await?;
    }

    // Update safe_search_enabled if provided
    if let Some(enabled) = body.safe_search_enabled {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES ('safe_search_enabled', $1) ON CONFLICT (key) DO UPDATE SET value = $1",
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
            "INSERT INTO settings (key, value) VALUES ('parental_control_enabled', $1) ON CONFLICT (key) DO UPDATE SET value = $1",
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
            "INSERT INTO settings (key, value) VALUES ('parental_control_level', $1) ON CONFLICT (key) DO UPDATE SET value = $1",
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

    // Note: Upstreams are managed by the /upstreams sub-router API.
    // We acknowledge update request but just warn.
    if body.upstreams.is_some() {
        tracing::warn!(
            "upstreams update requested but ignored (managed by /api/v1/settings/upstreams)"
        );
    }

    // Update ACL if provided
    if body.acl_allowed_networks.is_some() || body.acl_denied_networks.is_some() {
        // Read current values to apply partial updates
        let current_allowed: Vec<String> = sqlx::query_scalar::<_, String>(
            "SELECT value FROM settings WHERE key = 'acl_allowed_networks'",
        )
        .fetch_optional(&state.db)
        .await?
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default();

        let current_denied: Vec<String> = sqlx::query_scalar::<_, String>(
            "SELECT value FROM settings WHERE key = 'acl_denied_networks'",
        )
        .fetch_optional(&state.db)
        .await?
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default();

        let new_allowed = body.acl_allowed_networks.unwrap_or(current_allowed);
        let new_denied = body.acl_denied_networks.unwrap_or(current_denied);

        // Validate CIDR entries
        let bad_allowed = Acl::validate_cidrs(&new_allowed);
        let bad_denied = Acl::validate_cidrs(&new_denied);
        if !bad_allowed.is_empty() || !bad_denied.is_empty() {
            let mut bad: Vec<String> = bad_allowed;
            bad.extend(bad_denied);
            return Err(AppError::Validation(format!(
                "Invalid CIDR entries: {}",
                bad.join(", ")
            )));
        }

        let allowed_json = serde_json::to_string(&new_allowed).unwrap_or_else(|_| "[]".to_string());
        let denied_json = serde_json::to_string(&new_denied).unwrap_or_else(|_| "[]".to_string());

        sqlx::query("INSERT INTO settings (key, value) VALUES ('acl_allowed_networks', $1) ON CONFLICT (key) DO UPDATE SET value = $1")
            .bind(&allowed_json)
            .execute(&state.db)
            .await?;

        sqlx::query("INSERT INTO settings (key, value) VALUES ('acl_denied_networks', $1) ON CONFLICT (key) DO UPDATE SET value = $1")
            .bind(&denied_json)
            .execute(&state.db)
            .await?;

        // Hot-reload ACL in DNS handler
        state.dns_handler.reload_acl(new_allowed, new_denied).await;
    }

    if let Some(strategy) = body.upstream_strategy {
        if !matches!(strategy.as_str(), "priority" | "fastest" | "load_balance") {
            return Err(AppError::Validation(
                "upstream_strategy must be one of: priority, fastest, load_balance".to_string(),
            ));
        }
        sqlx::query("INSERT INTO settings (key, value) VALUES ('upstream_strategy', $1) ON CONFLICT (key) DO UPDATE SET value = $1")
            .bind(&strategy)
            .execute(&state.db)
            .await?;

        // Reload upstream pool mapping to apply the new strategy
        if let Err(e) = state.dns_handler.reload_upstreams().await {
            tracing::error!(
                "Failed to reload upstream pool after strategy change: {}",
                e
            );
        }
    }

    crate::db::audit::log_action(
        state.db.clone(),
        admin.0.sub.clone(),
        admin.0.username.clone(),
        "update",
        "settings",
        None,
        None,
        ip,
    );

    Ok(Json(json!({"success": true})))
}
