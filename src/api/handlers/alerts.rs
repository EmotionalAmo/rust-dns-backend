use crate::api::middleware::{auth::AuthUser, rbac::AdminUser};
use crate::api::AppState;
use crate::db::models::alert::Alert;
use crate::error::{AppError, AppResult};
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
    pub alert_type: Option<String>,
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

    let mut where_clauses = Vec::new();
    let mut param_idx = 1usize;

    if params.is_read.is_some() {
        where_clauses.push(format!("is_read = ${}", param_idx));
        param_idx += 1;
    }
    if params.alert_type.is_some() {
        where_clauses.push(format!("alert_type = ${}", param_idx));
        param_idx += 1;
    }

    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", where_clauses.join(" AND "))
    };

    let limit_idx = param_idx;
    let offset_idx = param_idx + 1;

    let query = format!(
        "SELECT id, alert_type, client_id, message, is_read, created_at FROM alerts{} ORDER BY created_at DESC LIMIT ${} OFFSET ${}",
        where_sql, limit_idx, offset_idx
    );
    let count_query = format!("SELECT COUNT(*) FROM alerts{}", where_sql);

    // Build count query with bound params
    let mut count_q = sqlx::query_scalar::<_, i64>(&count_query);
    if let Some(is_read) = params.is_read {
        count_q = count_q.bind(if is_read { 1i32 } else { 0i32 });
    }
    if let Some(ref alert_type) = params.alert_type {
        count_q = count_q.bind(alert_type.clone());
    }
    let total: i64 = count_q.fetch_one(&state.db).await.unwrap_or(0);

    // Build data query with bound params
    let mut data_q =
        sqlx::query_as::<_, (String, String, Option<String>, String, i32, String)>(&query);
    if let Some(is_read) = params.is_read {
        data_q = data_q.bind(if is_read { 1i32 } else { 0i32 });
    }
    if let Some(ref alert_type) = params.alert_type {
        data_q = data_q.bind(alert_type.clone());
    }
    let rows: Vec<(String, String, Option<String>, String, i32, String)> = data_q
        .bind(page_size)
        .bind(offset)
        .fetch_all(&state.db)
        .await?;

    let data: Vec<Alert> = rows
        .into_iter()
        .map(
            |(id, alert_type, client_id, message, is_read, created_at)| Alert {
                id,
                alert_type,
                client_id,
                message,
                is_read,
                created_at,
            },
        )
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
    let result = sqlx::query("UPDATE alerts SET is_read = 1 WHERE id = $1")
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
    let result = sqlx::query("DELETE FROM alerts").execute(&state.db).await?;

    Ok(Json(json!({
        "message": "Alerts cleared",
        "deleted": result.rows_affected()
    })))
}

/// Delete a single alert by ID
pub async fn delete_alert(
    State(state): State<Arc<AppState>>,
    _auth: AdminUser,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    let result = sqlx::query("DELETE FROM alerts WHERE id = $1")
        .bind(&id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Alert not found".into()));
    }

    Ok(Json(json!({ "message": "Alert deleted" })))
}
