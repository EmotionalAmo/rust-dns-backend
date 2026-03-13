use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::api::middleware::rbac::AdminUser;
use crate::api::AppState;
use crate::error::AppResult;

#[derive(Deserialize)]
pub struct AuditLogParams {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    pub user_id: Option<String>,
    pub action: Option<String>,
    pub resource: Option<String>,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Query(params): Query<AuditLogParams>,
) -> AppResult<Json<Value>> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(50).clamp(1, 200);
    let offset = (page - 1) * per_page;

    // Build dynamic WHERE clause
    let mut conditions: Vec<String> = Vec::new();
    if params.user_id.is_some() {
        conditions.push("user_id = ?".to_string());
    }
    if params.action.is_some() {
        conditions.push("action = ?".to_string());
    }
    if params.resource.is_some() {
        conditions.push("resource = ?".to_string());
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    // Count total
    let count_sql = pg_numbered(&format!("SELECT COUNT(*) FROM audit_log {}", where_clause));
    let mut count_query = sqlx::query_as::<_, (i64,)>(&count_sql);
    if let Some(ref v) = params.user_id {
        count_query = count_query.bind(v);
    }
    if let Some(ref v) = params.action {
        count_query = count_query.bind(v);
    }
    if let Some(ref v) = params.resource {
        count_query = count_query.bind(v);
    }
    let (total,) = count_query.fetch_one(&state.db).await?;

    // Fetch rows
    let data_sql = pg_numbered(&format!(
        "SELECT id, time, user_id, username, action, resource, resource_id, detail, ip \
         FROM audit_log {} ORDER BY time DESC LIMIT ? OFFSET ?",
        where_clause
    ));
    let mut data_query = sqlx::query_as::<
        _,
        (
            i64,
            String,
            String,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            String,
        ),
    >(&data_sql);
    if let Some(ref v) = params.user_id {
        data_query = data_query.bind(v);
    }
    if let Some(ref v) = params.action {
        data_query = data_query.bind(v);
    }
    if let Some(ref v) = params.resource {
        data_query = data_query.bind(v);
    }
    data_query = data_query.bind(per_page).bind(offset);
    let rows = data_query.fetch_all(&state.db).await?;

    let data: Vec<Value> = rows
        .into_iter()
        .map(
            |(id, time, user_id, username, action, resource, resource_id, detail, ip)| {
                json!({
                    "id": id,
                    "time": time,
                    "user_id": user_id,
                    "username": username,
                    "action": action,
                    "resource": resource,
                    "resource_id": resource_id,
                    "detail": detail,
                    "ip": ip,
                })
            },
        )
        .collect();

    Ok(Json(json!({
        "data": data,
        "total": total,
        "page": page,
        "per_page": per_page,
    })))
}

/// Convert SQLite-style `?` placeholders to PostgreSQL-style `$1`, `$2`, ...
fn pg_numbered(sql: &str) -> String {
    let mut result = sql.to_string();
    let mut n = 0usize;
    while let Some(pos) = result.find('?') {
        n += 1;
        result.replace_range(pos..pos + 1, &format!("${}", n));
    }
    result
}
