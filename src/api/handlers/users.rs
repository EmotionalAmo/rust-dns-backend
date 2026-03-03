use axum::{
    extract::{Path, State},
    Json,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

use crate::api::middleware::client_ip::ClientIp;
use crate::api::middleware::rbac::AdminUser;
use crate::api::AppState;
use crate::auth::password;
use crate::error::{AppError, AppResult};

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub role: String, // super_admin, admin, operator, read_only
}

#[derive(Debug, Deserialize)]
pub struct UpdateRoleRequest {
    pub role: String,
}

fn validate_role(role: &str) -> AppResult<()> {
    match role {
        "super_admin" | "admin" | "operator" | "read_only" => Ok(()),
        _ => Err(AppError::Validation(format!(
            "Invalid role: {}. Must be one of: super_admin, admin, operator, read_only",
            role
        ))),
    }
}

pub async fn list(State(state): State<Arc<AppState>>, _admin: AdminUser) -> AppResult<Json<Value>> {
    let rows: Vec<(String, String, String, i64, String, String)> = sqlx::query_as(
        "SELECT id, username, role, is_active, created_at, updated_at
         FROM users ORDER BY created_at DESC",
    )
    .fetch_all(&state.db)
    .await?;

    let data: Vec<Value> = rows
        .into_iter()
        .map(|(id, username, role, is_active, created_at, updated_at)| {
            json!({
                "id": id,
                "username": username,
                "role": role,
                "is_active": is_active == 1,
                "created_at": created_at,
                "updated_at": updated_at,
            })
        })
        .collect();
    let count = data.len();
    Ok(Json(json!({ "data": data, "total": count })))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    ClientIp(ip): ClientIp,
    admin: AdminUser,
    Json(body): Json<CreateUserRequest>,
) -> AppResult<Json<Value>> {
    // Validate role
    validate_role(&body.role)?;

    let username = body.username.trim().to_string();
    if username.is_empty() {
        return Err(AppError::Validation("Username cannot be empty".to_string()));
    }

    if body.password.len() < 8 {
        return Err(AppError::Validation(
            "Password must be at least 8 characters".to_string(),
        ));
    }

    // Check if username already exists
    let existing: Option<(String,)> =
        sqlx::query_as("SELECT username FROM users WHERE username = ?")
            .bind(&username)
            .fetch_optional(&state.db)
            .await?;

    if existing.is_some() {
        return Err(AppError::Validation(format!(
            "Username '{}' already exists",
            username
        )));
    }

    let id = Uuid::new_v4().to_string();
    let password_hash = password::hash(&body.password)
        .map_err(|e| AppError::Internal(format!("Password hashing failed: {}", e)))?;
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO users (id, username, password, role, is_active, created_at, updated_at)
         VALUES (?, ?, ?, ?, 1, ?, ?)",
    )
    .bind(&id)
    .bind(&username)
    .bind(&password_hash)
    .bind(&body.role)
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await?;

    crate::db::audit::log_action(
        state.db.clone(),
        admin.0.sub.clone(),
        admin.0.username.clone(),
        "create",
        "user",
        Some(id.clone()),
        Some(format!("username={}, role={}", username, body.role)),
        ip,
    );

    Ok(Json(json!({
        "id": id,
        "username": username,
        "role": body.role,
        "is_active": true,
        "created_at": now,
        "updated_at": now,
    })))
}

pub async fn update_role(
    State(state): State<Arc<AppState>>,
    ClientIp(ip): ClientIp,
    admin: AdminUser,
    Path(id): Path<String>,
    Json(body): Json<UpdateRoleRequest>,
) -> AppResult<Json<Value>> {
    // Validate role
    validate_role(&body.role)?;

    // Check if user exists
    let existing: Option<(String, String, String, i64, String, String)> = sqlx::query_as(
        "SELECT id, username, role, is_active, created_at, updated_at
         FROM users WHERE id = ?",
    )
    .bind(&id)
    .fetch_optional(&state.db)
    .await?;

    let (_, username, old_role, is_active, created_at, _updated_at) =
        existing.ok_or_else(|| AppError::NotFound(format!("User {} not found", id)))?;

    // Prevent modifying the last super_admin
    if old_role == "super_admin" && body.role != "super_admin" {
        let super_admin_count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM users WHERE role = 'super_admin' AND is_active = 1",
        )
        .fetch_one(&state.db)
        .await?;

        if super_admin_count.0 <= 1 {
            return Err(AppError::Validation(
                "Cannot change role: at least one super_admin must remain".to_string(),
            ));
        }
    }

    let now = Utc::now().to_rfc3339();

    sqlx::query("UPDATE users SET role = ?, updated_at = ? WHERE id = ?")
        .bind(&body.role)
        .bind(&now)
        .bind(&id)
        .execute(&state.db)
        .await?;

    crate::db::audit::log_action(
        state.db.clone(),
        admin.0.sub.clone(),
        admin.0.username.clone(),
        "update_role",
        "user",
        Some(id.clone()),
        Some(format!("username={}, role={}->{}", username, old_role, body.role)),
        ip,
    );

    Ok(Json(json!({
        "id": id,
        "username": username,
        "role": body.role,
        "is_active": is_active == 1,
        "created_at": created_at,
        "updated_at": now,
    })))
}
