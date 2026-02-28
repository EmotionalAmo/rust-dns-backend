use crate::api::middleware::rbac::AdminUser;
use crate::api::AppState;
use crate::error::AppResult;
use axum::{extract::State, http::StatusCode, Json};
use serde_json::{json, Value};
use std::sync::Arc;

pub async fn get_cache_stats(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> AppResult<Json<Value>> {
    let (entry_count, max_capacity) = state.dns_handler.cache_stats().await;
    Ok(Json(json!({
        "entry_count": entry_count,
        "max_capacity": max_capacity,
        "usage_percent": (entry_count as f64 / max_capacity as f64 * 100.0).round()
    })))
}

pub async fn flush_cache(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> AppResult<(StatusCode, Json<Value>)> {
    state.dns_handler.cache_flush().await;
    Ok((
        StatusCode::OK,
        Json(json!({"success": true, "message": "DNS cache flushed"})),
    ))
}
