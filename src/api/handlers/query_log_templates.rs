// Query Log Templates CRUD
// File: src/api/handlers/query_log_templates.rs
// Author: ui-duarte
// Date: 2026-02-20

use crate::api::middleware::auth::AuthUser;
use crate::api::AppState;
use crate::error::AppError;
use crate::error::AppResult;
use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

// ============================================================================
// Data Structures
// ============================================================================

#[derive(Debug, Serialize)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub filters: serde_json::Value,
    pub logic: String,
    pub created_by: String,
    pub created_at: String,
    pub is_public: bool,
}

#[derive(Debug, Deserialize)]
pub struct TemplateCreate {
    pub name: String,
    pub filters: serde_json::Value,
    pub logic: Option<String>,
    pub is_public: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct TemplateUpdate {
    pub name: Option<String>,
    pub filters: Option<serde_json::Value>,
    pub logic: Option<String>,
    pub is_public: Option<bool>,
}

// ============================================================================
// API Handlers
// ============================================================================

/// 列出所有查询模板（公开的 + 自己创建的）
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> AppResult<Json<Vec<Template>>> {
    let rows = sqlx::query_as::<_, (String, String, String, String, String, String, bool)>(
        "SELECT id, name, filters, logic, created_by, created_at, is_public
         FROM query_log_templates
         WHERE is_public = true OR created_by = $1
         ORDER BY created_at DESC",
    )
    .bind(&auth.0.username)
    .fetch_all(&state.db)
    .await?;

    let templates: Vec<Template> = rows
        .into_iter()
        .map(
            |(id, name, filters, logic, created_by, created_at, is_public)| Template {
                id,
                name,
                filters: serde_json::from_str(&filters).unwrap_or(json!([])),
                logic,
                created_by,
                created_at,
                is_public,
            },
        )
        .collect();

    Ok(Json(templates))
}

/// 创建查询模板
pub async fn create(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<TemplateCreate>,
) -> AppResult<Json<Template>> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let logic = req.logic.unwrap_or_else(|| "AND".to_string());
    let is_public = req.is_public.unwrap_or(false);
    let filters_json = serde_json::to_string(&req.filters)
        .map_err(|e| anyhow::anyhow!("Invalid filters JSON: {}", e))?;

    sqlx::query(
        "INSERT INTO query_log_templates (id, name, filters, logic, created_by, created_at, is_public)
         VALUES ($1, $2, $3, $4, $5, $6, $7)"
    )
    .bind(&id)
    .bind(&req.name)
    .bind(&filters_json)
    .bind(&logic)
    .bind(&auth.0.username)
    .bind(&now)
    .bind(is_public)
    .execute(&state.db)
    .await?;

    Ok(Json(Template {
        id,
        name: req.name,
        filters: req.filters,
        logic,
        created_by: auth.0.username,
        created_at: now,
        is_public,
    }))
}

/// 获取单个模板
pub async fn get(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<Template>> {
    let row = sqlx::query_as::<_, (String, String, String, String, String, String, bool)>(
        "SELECT id, name, filters, logic, created_by, created_at, is_public
         FROM query_log_templates
         WHERE id = $1 AND (is_public = true OR created_by = $2)",
    )
    .bind(&id)
    .bind(&auth.0.username)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("Template not found or access denied".to_string()))?;

    let (id, name, filters, logic, created_by, created_at, is_public) = row;
    Ok(Json(Template {
        id,
        name,
        filters: serde_json::from_str(&filters).unwrap_or(json!([])),
        logic,
        created_by,
        created_at,
        is_public,
    }))
}

/// 更新模板（仅创建者可编辑）
pub async fn update(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(req): Json<TemplateUpdate>,
) -> AppResult<Json<Template>> {
    // 权限检查：必须是创建者
    let owner: Option<String> =
        sqlx::query_scalar("SELECT created_by FROM query_log_templates WHERE id = $1")
            .bind(&id)
            .fetch_optional(&state.db)
            .await?
            .ok_or_else(|| AppError::NotFound("Template not found".to_string()))?;

    if owner.as_ref() != Some(&auth.0.username) {
        return Err(AppError::Unauthorized("you are not the owner".to_string()));
    }

    // 构建动态更新语句
    let mut updates = Vec::new();
    let mut bindings: Vec<String> = Vec::new();
    let mut param_idx = 1;

    if let Some(name) = req.name {
        updates.push(format!("name = ${}", param_idx));
        bindings.push(name);
        param_idx += 1;
    }
    if let Some(filters) = req.filters {
        updates.push(format!("filters = ${}", param_idx));
        let filters_json = serde_json::to_string(&filters)
            .map_err(|e| anyhow::anyhow!("Invalid filters JSON: {}", e))?;
        bindings.push(filters_json);
        param_idx += 1;
    }
    if let Some(logic) = req.logic {
        updates.push(format!("logic = ${}", param_idx));
        bindings.push(logic);
        param_idx += 1;
    }
    if let Some(is_public) = req.is_public {
        updates.push(format!("is_public = ${}", param_idx));
        bindings.push(is_public.to_string());
        param_idx += 1;
    }

    if updates.is_empty() {
        return Err(AppError::Validation("No fields to update".to_string()));
    }

    let sql = format!(
        "UPDATE query_log_templates SET {} WHERE id = ${}",
        updates.join(", "),
        param_idx
    );

    // Execute update
    let mut q = sqlx::query(&sql);
    for binding in bindings {
        q = q.bind(binding);
    }
    q = q.bind(&id);
    q.execute(&state.db).await?;

    // Fetch updated template
    let get_req = get(State(state.clone()), auth, Path(id.clone()));
    get_req.await
}

/// 删除模板（仅创建者可删除）
pub async fn delete(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> AppResult<impl IntoResponse> {
    let owner: Option<String> =
        sqlx::query_scalar("SELECT created_by FROM query_log_templates WHERE id = $1")
            .bind(&id)
            .fetch_optional(&state.db)
            .await?
            .ok_or_else(|| AppError::NotFound("Template not found".to_string()))?;

    if owner.as_ref() != Some(&auth.0.username) {
        return Err(AppError::Unauthorized("you are not the owner".to_string()));
    }

    sqlx::query("DELETE FROM query_log_templates WHERE id = $1")
        .bind(&id)
        .execute(&state.db)
        .await?;

    Ok(Json(json!({ "message": "Template deleted successfully" })))
}
