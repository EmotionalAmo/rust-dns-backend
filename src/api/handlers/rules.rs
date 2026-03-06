use crate::api::middleware::auth::AuthUser;
use crate::api::middleware::client_ip::ClientIp;
use crate::api::AppState;
use crate::error::{AppError, AppResult};
use axum::{
    extract::{Multipart, Path, Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct CreateRuleRequest {
    rule: String,
    #[serde(default)]
    comment: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateRuleRequest {
    rule: Option<String>,
    comment: Option<String>,
    #[serde(default)]
    is_enabled: Option<bool>,
}

#[derive(Deserialize)]
pub struct ToggleRuleRequest {
    #[serde(default)]
    is_enabled: bool,
}

#[derive(Deserialize)]
pub struct ListParams {
    page: Option<u32>,
    per_page: Option<u32>,
    search: Option<String>,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    #[allow(unused_variables)] ClientIp(ip): ClientIp,
    Query(params): Query<ListParams>,
    _auth: AuthUser,
) -> AppResult<Json<Value>> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(50).min(200) as i64;
    let offset = (page as i64 - 1) * per_page;
    let search = params.search.as_deref().unwrap_or("").trim().to_string();
    let has_search = !search.is_empty();
    let search_pattern = format!("%{}%", search);

    // 只显示用户手动创建的规则，过滤掉订阅列表导入的规则（created_by LIKE 'filter:%'）
    let where_clause = if has_search {
        "WHERE created_by NOT LIKE 'filter:%' AND (rule LIKE ? OR comment LIKE ?)"
    } else {
        "WHERE created_by NOT LIKE 'filter:%'"
    };

    let count_sql = format!("SELECT COUNT(*) FROM custom_rules {}", where_clause);
    let data_sql = format!(
        "SELECT id, rule, comment, is_enabled, created_by, created_at \
         FROM custom_rules {} ORDER BY created_at DESC LIMIT ? OFFSET ?",
        where_clause
    );

    let total: i64 = if has_search {
        sqlx::query_scalar(&count_sql)
            .bind(&search_pattern)
            .bind(&search_pattern)
            .fetch_one(&state.db)
            .await?
    } else {
        sqlx::query_scalar(&count_sql).fetch_one(&state.db).await?
    };

    let rows: Vec<(String, String, Option<String>, i64, String, String)> = if has_search {
        sqlx::query_as(&data_sql)
            .bind(&search_pattern)
            .bind(&search_pattern)
            .bind(per_page)
            .bind(offset)
            .fetch_all(&state.db)
            .await?
    } else {
        sqlx::query_as(&data_sql)
            .bind(per_page)
            .bind(offset)
            .fetch_all(&state.db)
            .await?
    };

    let data: Vec<Value> = rows
        .into_iter()
        .map(|(id, rule, comment, is_enabled, created_by, created_at)| {
            json!({
                "id": id,
                "rule": rule,
                "comment": comment,
                "is_enabled": is_enabled == 1,
                "created_by": created_by,
                "created_at": created_at,
            })
        })
        .collect();

    Ok(Json(json!({
        "data": data,
        "total": total,
        "page": page,
        "per_page": per_page,
    })))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    #[allow(unused_variables)] ClientIp(ip): ClientIp,
    auth: AuthUser,
    Json(body): Json<CreateRuleRequest>,
) -> AppResult<Json<Value>> {
    let rule = body.rule.trim().to_string();
    if rule.is_empty() {
        return Err(AppError::Validation("Rule cannot be empty".to_string()));
    }

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO custom_rules (id, rule, comment, is_enabled, created_by, created_at)
         VALUES (?, ?, ?, 1, ?, ?)",
    )
    .bind(&id)
    .bind(&rule)
    .bind(&body.comment)
    .bind(&auth.0.username)
    .bind(&now)
    .execute(&state.db)
    .await?;

    state
        .filter
        .reload()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    crate::db::audit::log_action(
        state.db.clone(),
        auth.0.sub.clone(),
        auth.0.username.clone(),
        "create",
        "rule",
        Some(id.clone()),
        Some(rule.clone()),
        ip.clone(),
    );

    Ok(Json(json!({
        "id": id,
        "rule": rule,
        "comment": body.comment,
        "is_enabled": true,
        "created_by": auth.0.username,
        "created_at": now,
    })))
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    #[allow(unused_variables)] ClientIp(ip): ClientIp,
    auth: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    let result = sqlx::query("DELETE FROM custom_rules WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("Rule {} not found", id)));
    }

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
        "rule",
        Some(id.clone()),
        None,
        ip.clone(),
    );

    Ok(Json(json!({"success": true})))
}

#[derive(Deserialize)]
pub struct BulkActionRequest {
    pub ids: Vec<String>,
    pub action: String, // "enable", "disable", "delete"
}

pub async fn bulk_action(
    State(state): State<Arc<AppState>>,
    #[allow(unused_variables)] ClientIp(ip): ClientIp,
    auth: AuthUser,
    Json(req): Json<BulkActionRequest>,
) -> AppResult<Json<Value>> {
    if req.ids.is_empty() {
        return Ok(Json(json!({"affected": 0})));
    }

    let placeholders = req.ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");

    let affected: u64 = match req.action.as_str() {
        "enable" => {
            let sql = format!(
                "UPDATE custom_rules SET is_enabled = 1 WHERE id IN ({}) AND created_by NOT LIKE 'filter:%'",
                placeholders
            );
            let mut q = sqlx::query(&sql);
            for id in &req.ids {
                q = q.bind(id);
            }
            q.execute(&state.db).await?.rows_affected()
        }
        "disable" => {
            let sql = format!(
                "UPDATE custom_rules SET is_enabled = 0 WHERE id IN ({}) AND created_by NOT LIKE 'filter:%'",
                placeholders
            );
            let mut q = sqlx::query(&sql);
            for id in &req.ids {
                q = q.bind(id);
            }
            q.execute(&state.db).await?.rows_affected()
        }
        "delete" => {
            let sql = format!(
                "DELETE FROM custom_rules WHERE id IN ({}) AND created_by NOT LIKE 'filter:%'",
                placeholders
            );
            let mut q = sqlx::query(&sql);
            for id in &req.ids {
                q = q.bind(id);
            }
            q.execute(&state.db).await?.rows_affected()
        }
        _ => {
            return Err(AppError::Validation(
                "action must be enable, disable, or delete".to_string(),
            ))
        }
    };

    state
        .filter
        .reload()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    crate::db::audit::log_action(
        state.db.clone(),
        auth.0.sub.clone(),
        auth.0.username.clone(),
        "bulk_action",
        "rule",
        None,
        Some(format!("action={}, count={}", req.action, affected)),
        ip.clone(),
    );

    Ok(Json(json!({"affected": affected})))
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    #[allow(unused_variables)] ClientIp(ip): ClientIp,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<UpdateRuleRequest>,
) -> AppResult<Json<Value>> {
    // 获取现有规则
    let row = sqlx::query_as::<_, (String, Option<String>, i64, String)>(
        "SELECT rule, comment, is_enabled, created_by FROM custom_rules WHERE id = ?",
    )
    .bind(&id)
    .fetch_optional(&state.db)
    .await?;

    if row.is_none() {
        return Err(AppError::NotFound(format!("Rule {} not found", id)));
    }

    let (_existing_rule, _existing_comment, existing_is_enabled, created_by) = row.unwrap();

    // 只能更新用户手动创建的规则
    if created_by.starts_with("filter:") {
        return Err(AppError::Unauthorized(
            "Cannot edit rules imported from filter lists".to_string(),
        ));
    }

    // 构建更新 SQL
    let mut updates = Vec::new();
    let mut needs_reload = false;

    if let Some(rule) = &body.rule {
        if rule.trim().is_empty() {
            return Err(AppError::Validation("Rule cannot be empty".to_string()));
        }
        updates.push("rule = ?");
    }

    if body.comment.is_some() {
        updates.push("comment = ?");
    }

    if let Some(is_enabled) = body.is_enabled {
        updates.push("is_enabled = ?");
        if existing_is_enabled != (if is_enabled { 1 } else { 0 }) {
            needs_reload = true;
        }
    }

    if updates.is_empty() {
        return Ok(Json(json!({"success": true, "updated": false})));
    }

    let sql = format!(
        "UPDATE custom_rules SET {} WHERE id = ?",
        updates.join(", ")
    );

    let mut query = sqlx::query(&sql);
    if let Some(rule) = &body.rule {
        query = query.bind(rule.trim());
    }
    if let Some(comment) = &body.comment {
        query = query.bind(comment);
    }
    if let Some(is_enabled) = body.is_enabled {
        query = query.bind(if is_enabled { 1 } else { 0 });
    }
    query = query.bind(&id);

    let result = query.execute(&state.db).await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("Rule {} not found", id)));
    }

    // 如果状态改变，需要重新加载过滤器
    if needs_reload || body.rule.is_some() {
        state
            .filter
            .reload()
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;
    }

    // 返回更新后的规则
    let updated = sqlx::query_as::<_, (String, String, Option<String>, i64, String, String)>(
        "SELECT id, rule, comment, is_enabled, created_by, created_at FROM custom_rules WHERE id = ?"
    )
    .bind(&id)
    .fetch_one(&state.db)
    .await?;

    crate::db::audit::log_action(
        state.db.clone(),
        auth.0.sub.clone(),
        auth.0.username.clone(),
        "update",
        "rule",
        Some(id.clone()),
        None,
        ip.clone(),
    );

    Ok(Json(json!({
        "id": updated.0,
        "rule": updated.1,
        "comment": updated.2,
        "is_enabled": updated.3 == 1,
        "created_by": updated.4,
        "created_at": updated.5,
    })))
}

pub async fn toggle(
    State(state): State<Arc<AppState>>,
    #[allow(unused_variables)] ClientIp(ip): ClientIp,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<ToggleRuleRequest>,
) -> AppResult<Json<Value>> {
    let result = sqlx::query(
        "UPDATE custom_rules SET is_enabled = ? WHERE id = ? AND created_by NOT LIKE 'filter:%'",
    )
    .bind(if body.is_enabled { 1 } else { 0 })
    .bind(&id)
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!(
            "Rule {} not found or is a filter list rule",
            id
        )));
    }

    state
        .filter
        .reload()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    crate::db::audit::log_action(
        state.db.clone(),
        auth.0.sub.clone(),
        auth.0.username.clone(),
        "toggle",
        "rule",
        Some(id.clone()),
        Some(format!("is_enabled={}", body.is_enabled)),
        ip.clone(),
    );

    Ok(Json(json!({
        "id": id,
        "is_enabled": body.is_enabled,
    })))
}

#[derive(Deserialize)]
pub struct ExportParams {
    #[serde(default = "default_export_format")]
    format: String,
}

fn default_export_format() -> String {
    "json".to_string()
}

pub async fn export_rules(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ExportParams>,
    _auth: AuthUser,
) -> impl IntoResponse {
    let rows: Vec<(String, String, Option<String>, i64, String, String)> = match sqlx::query_as(
        "SELECT id, rule, comment, is_enabled, created_by, created_at \
         FROM custom_rules \
         WHERE created_by NOT LIKE 'filter:%' \
         ORDER BY created_at DESC",
    )
    .fetch_all(&state.db)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Rules export DB query failed: {}", e);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Database error during export",
                    "detail": e.to_string()
                })),
            )
                .into_response();
        }
    };

    match params.format.as_str() {
        "csv" => {
            let mut csv = String::new();
            csv.push_str("id,rule,comment,is_enabled,created_by,created_at\n");

            for (id, rule, comment, is_enabled, created_by, created_at) in &rows {
                let comment_str = comment.as_deref().unwrap_or("");
                csv.push_str(&format!(
                    "{},{},{},{},{},{}\n",
                    escape_csv_field(id),
                    escape_csv_field(rule),
                    escape_csv_field(comment_str),
                    if *is_enabled == 1 { "true" } else { "false" },
                    escape_csv_field(created_by),
                    escape_csv_field(created_at),
                ));
            }

            (
                [
                    (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
                    (
                        header::CONTENT_DISPOSITION,
                        "attachment; filename=\"rules_export.csv\"",
                    ),
                ],
                csv,
            )
                .into_response()
        }
        _ => {
            // json format (default)
            let data: Vec<Value> = rows
                .into_iter()
                .map(|(id, rule, comment, is_enabled, created_by, created_at)| {
                    json!({
                        "id": id,
                        "rule": rule,
                        "comment": comment,
                        "is_enabled": is_enabled == 1,
                        "created_by": created_by,
                        "created_at": created_at,
                    })
                })
                .collect();

            let body = serde_json::to_string_pretty(&data).unwrap_or_default();
            (
                [
                    (header::CONTENT_TYPE, "application/json"),
                    (
                        header::CONTENT_DISPOSITION,
                        "attachment; filename=\"rules_export.json\"",
                    ),
                ],
                body,
            )
                .into_response()
        }
    }
}

fn escape_csv_field(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

// Maximum allowed upload size: 5 MB
const MAX_IMPORT_SIZE: usize = 5 * 1024 * 1024;

pub async fn import_rules(
    State(state): State<Arc<AppState>>,
    ClientIp(ip): ClientIp,
    auth: AuthUser,
    mut multipart: Multipart,
) -> impl IntoResponse {
    // --- 1. Read file field from multipart ---
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut content_type_hint: Option<String> = None;
    let mut filename_hint: Option<String> = None;

    loop {
        match multipart.next_field().await {
            Ok(Some(field)) => {
                if field.name() != Some("file") {
                    continue;
                }
                filename_hint = field.file_name().map(|s| s.to_lowercase());
                content_type_hint = field.content_type().map(|ct| ct.to_string());

                match field.bytes().await {
                    Ok(bytes) => {
                        if bytes.len() > MAX_IMPORT_SIZE {
                            return (
                                StatusCode::PAYLOAD_TOO_LARGE,
                                Json(json!({
                                    "error": "File too large. Maximum allowed size is 5 MB."
                                })),
                            )
                                .into_response();
                        }
                        file_bytes = Some(bytes.to_vec());
                        break;
                    }
                    Err(e) => {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(json!({ "error": format!("Failed to read file: {}", e) })),
                        )
                            .into_response();
                    }
                }
            }
            Ok(None) => break,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("Multipart error: {}", e) })),
                )
                    .into_response();
            }
        }
    }

    let bytes = match file_bytes {
        Some(b) if !b.is_empty() => b,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "No file uploaded or file is empty." })),
            )
                .into_response();
        }
    };

    // --- 2. Detect format and parse rules ---
    let content = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "File is not valid UTF-8 text." })),
            )
                .into_response();
        }
    };

    let is_json = content_type_hint
        .as_deref()
        .map(|ct| ct.contains("application/json"))
        .unwrap_or(false)
        || filename_hint
            .as_deref()
            .map(|f| f.ends_with(".json"))
            .unwrap_or(false)
        || content.trim_start().starts_with('[')
        || content.trim_start().starts_with('{');

    struct RuleEntry {
        rule: String,
        comment: Option<String>,
    }

    let parsed: Result<Vec<RuleEntry>, String> = if is_json {
        // Try to parse as JSON array of objects or strings
        match serde_json::from_str::<Value>(content.trim()) {
            Ok(Value::Array(arr)) => {
                let mut entries = Vec::new();
                for item in arr {
                    match item {
                        Value::String(s) => {
                            let r = s.trim().to_string();
                            if !r.is_empty() {
                                entries.push(RuleEntry {
                                    rule: r,
                                    comment: None,
                                });
                            }
                        }
                        Value::Object(obj) => {
                            let rule = obj
                                .get("rule")
                                .and_then(|v| v.as_str())
                                .map(|s| s.trim().to_string())
                                .unwrap_or_default();
                            if rule.is_empty() {
                                continue;
                            }
                            let comment = obj
                                .get("comment")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            entries.push(RuleEntry { rule, comment });
                        }
                        _ => {}
                    }
                }
                Ok(entries)
            }
            Ok(_) => Err("JSON must be an array of strings or objects.".to_string()),
            Err(e) => Err(format!("Invalid JSON: {}", e)),
        }
    } else {
        // Plain text: one rule per line, skip empty lines and # comments
        let entries = content
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| RuleEntry {
                rule: line.to_string(),
                comment: None,
            })
            .collect();
        Ok(entries)
    };

    let entries = match parsed {
        Ok(e) => e,
        Err(msg) => {
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": msg }))).into_response();
        }
    };

    let total = entries.len();

    if total == 0 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "No valid rules found in the uploaded file." })),
        )
            .into_response();
    }

    // --- 3. Dedup against existing rules and batch insert ---
    let now = Utc::now().to_rfc3339();
    let username = auth.0.username.clone();
    let user_id = auth.0.sub.clone();

    let mut imported: usize = 0;
    let mut skipped: usize = 0;

    // Use a transaction for the batch insert
    let mut tx = match state.db.begin().await {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("Failed to begin transaction: {}", e) })),
            )
                .into_response();
        }
    };

    for entry in entries {
        let rule = entry.rule.trim().to_string();
        if rule.is_empty() {
            skipped += 1;
            continue;
        }

        // Check for duplicate
        let exists: Option<(String,)> =
            match sqlx::query_as("SELECT id FROM custom_rules WHERE rule = ?")
                .bind(&rule)
                .fetch_optional(&mut *tx)
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.rollback().await;
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "error": format!("Database error: {}", e) })),
                    )
                        .into_response();
                }
            };

        if exists.is_some() {
            skipped += 1;
            continue;
        }

        let id = Uuid::new_v4().to_string();
        if let Err(e) = sqlx::query(
            "INSERT INTO custom_rules (id, rule, comment, is_enabled, created_by, created_at)
             VALUES (?, ?, ?, 1, ?, ?)",
        )
        .bind(&id)
        .bind(&rule)
        .bind(&entry.comment)
        .bind(&username)
        .bind(&now)
        .execute(&mut *tx)
        .await
        {
            let _ = tx.rollback().await;
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("Insert failed: {}", e) })),
            )
                .into_response();
        }

        imported += 1;
    }

    if let Err(e) = tx.commit().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Failed to commit transaction: {}", e) })),
        )
            .into_response();
    }

    // --- 4. Reload filter if anything was imported ---
    if imported > 0 {
        if let Err(e) = state.filter.reload().await {
            tracing::error!("Filter reload failed after import: {}", e);
        }
    }

    // --- 5. Audit log ---
    crate::db::audit::log_action(
        state.db.clone(),
        user_id,
        username,
        "import",
        "rule",
        None,
        Some(format!(
            "imported={}, skipped={}, total={}",
            imported, skipped, total
        )),
        ip,
    );

    (
        StatusCode::OK,
        Json(json!({
            "imported": imported,
            "skipped": skipped,
            "total": total,
        })),
    )
        .into_response()
}
