use crate::error::{AppError, AppResult};
use crate::api::AppState;
use crate::api::middleware::{auth::AuthUser, rbac::AdminUser};
use crate::db::models::alert::Alert;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub struct AlertFilterParams {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub is_read: Option<bool>,
}

/// Get paginated alerts
pub async fn list_alerts(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(params): Query<AlertFilterParams>,
) -> AppResult<Json<Value>> {
    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(50).min(100);
    let offset = (page - 1) * page_size;

    let mut query = "SELECT id, alert_type, client_id, message, is_read, created_at FROM alerts".to_string();
    let mut count_query = "SELECT COUNT(*) FROM alerts".to_string();
    
    let mut where_clauses = Vec::new();
    if let Some(is_read) = params.is_read {
        let val = if is_read { 1 } else { 0 };
        where_clauses.push(format!("is_read = {}", val));
    }

    if !where_clauses.is_empty() {
        let clause = format!(" WHERE {}", where_clauses.join(" AND "));
        query.push_str(&clause);
        count_query.push_str(&clause);
    }

    query.push_str(" ORDER BY created_at DESC LIMIT ? OFFSET ?");

    let total: i64 = sqlx::query_scalar(&count_query)
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);

    let rows: Vec<(String, String, Option<String>, String, i32, String)> = sqlx::query_as(&query)
        .bind(page_size)
        .bind(offset)
        .fetch_all(&state.db)
        .await?;

    let data: Vec<Alert> = rows
        .into_iter()
        .map(|(id, alert_type, client_id, message, is_read, created_at)| Alert {
            id,
            alert_type,
            client_id,
            message,
            is_read: is_read,
            created_at,
        })
        .collect();

    Ok(Json(json!({
        "data": data,
        "total": total,
        "page": page,
        "page_size": page_size,
    })))
}

/// Mark a single alert as read
pub async fn mark_alert_read(
    State(state): State<Arc<AppState>>,
    _auth: AdminUser,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    let result = sqlx::query("UPDATE alerts SET is_read = 1 WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Alert not found".into()));
    }

    Ok(Json(json!({ "message": "Alert marked as read" })))
}

/// Mark all alerts as read
pub async fn mark_all_alerts_read(
    State(state): State<Arc<AppState>>,
    _auth: AdminUser,
) -> AppResult<Json<Value>> {
    let result = sqlx::query("UPDATE alerts SET is_read = 1 WHERE is_read = 0")
        .execute(&state.db)
        .await?;

    Ok(Json(json!({
        "message": "All alerts marked as read",
        "affected": result.rows_affected()
    })))
}

/// Clear all read alerts (or all alerts)
pub async fn clear_alerts(
    State(state): State<Arc<AppState>>,
    _auth: AdminUser,
) -> AppResult<Json<Value>> {
    let result = sqlx::query("DELETE FROM alerts")
        .execute(&state.db)
        .await?;

    Ok(Json(json!({
        "message": "Alerts cleared",
        "deleted": result.rows_affected()
    })))
}
