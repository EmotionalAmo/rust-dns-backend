use crate::api::middleware::auth::AuthUser;
use crate::api::middleware::client_ip::ClientIp;
use crate::api::AppState;
use crate::error::{AppError, AppResult};
use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Maximum login failures per IP within the window before lockout.
const MAX_LOGIN_FAILURES: u32 = 5;
/// Failure counting window (15 minutes).
const FAILURE_WINDOW: Duration = Duration::from_secs(15 * 60);

#[derive(Deserialize)]
pub struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    ClientIp(ip): ClientIp,
    Json(req): Json<LoginRequest>,
) -> AppResult<Json<Value>> {
    // Rate-limit: reject if this IP has too many recent failures (H-5 fix)
    {
        let mut should_reject = false;
        if let Some(mut entry) = state.login_attempts.get_mut(&ip) {
            let (count, window_start) = entry.value_mut();
            if window_start.elapsed() > FAILURE_WINDOW {
                // Window expired — reset
                *count = 0;
                *window_start = Instant::now();
            } else if *count >= MAX_LOGIN_FAILURES {
                should_reject = true;
            }
        }
        if should_reject {
            tracing::warn!("Login rate limit exceeded for IP: {}", ip);
            return Err(AppError::TooManyRequests);
        }
    }

    let row: Option<(String, String, String, i32)> =
        sqlx::query_as("SELECT id, password, role, is_active FROM users WHERE username = $1")
            .bind(&req.username)
            .fetch_optional(&state.db)
            .await?;

    let (user_id, password_hash, role, is_active) = match row {
        Some(r) => r,
        None => {
            // Record failure even for unknown usernames (prevent user enumeration timing)
            record_login_failure(&state, &ip);
            return Err(AppError::AuthFailed);
        }
    };

    if is_active == 0 {
        record_login_failure(&state, &ip);
        return Err(AppError::AuthFailed);
    }

    if !crate::auth::password::verify(&req.password, &password_hash) {
        record_login_failure(&state, &ip);
        return Err(AppError::AuthFailed);
    }

    // Successful login — clear failure counter for this IP
    state.login_attempts.remove(&ip);

    // Warn if using the initial bootstrap password (L-3: no named const in source)
    // In testing mode (allow_default_password=true), we skip the forced password change
    let is_default_password = req.password == "admin";
    let requires_password_change = is_default_password && !state.allow_default_password;
    if is_default_password && !state.allow_default_password {
        tracing::warn!(
            "User {} logged in with default password - force change required",
            req.username
        );
    } else if is_default_password {
        tracing::info!(
            "User {} logged in with default password (testing mode, change not required)",
            req.username
        );
    }

    // Enforce a hard maximum of 168 hours (7 days) for JWT security
    let expiry_hours = std::cmp::min(state.jwt_expiry_hours, 168);

    let token = crate::auth::jwt::generate(
        &user_id,
        &req.username,
        &role,
        &state.jwt_secret,
        expiry_hours,
    )
    .map_err(|e| AppError::Internal(e.to_string()))?;

    crate::db::audit::log_action(
        state.db.clone(),
        user_id.clone(),
        req.username.clone(),
        "login",
        "session",
        None,
        None,
        ip.clone(),
    );

    Ok(Json(json!({
        "token": token,
        "expires_in": state.jwt_expiry_hours * 3600,
        "role": role,
        "requires_password_change": requires_password_change,
    })))
}

fn record_login_failure(state: &AppState, ip: &str) {
    let mut entry = state
        .login_attempts
        .entry(ip.to_string())
        .or_insert_with(|| (0, Instant::now()));
    let (count, window_start) = entry.value_mut();
    if window_start.elapsed() > FAILURE_WINDOW {
        *count = 1;
        *window_start = Instant::now();
    } else {
        *count += 1;
    }
}

pub async fn logout() -> AppResult<Json<Value>> {
    // JWT is stateless; client just discards the token.
    Ok(Json(json!({"success": true})))
}

pub async fn change_password(
    State(state): State<Arc<AppState>>,
    ClientIp(ip): ClientIp,
    auth: AuthUser,
    Json(req): Json<ChangePasswordRequest>,
) -> AppResult<Json<Value>> {
    // Access the Claims from AuthUser tuple struct
    let claims = auth.0;

    // Validate new password length
    if req.new_password.len() < 8 {
        return Err(AppError::Validation(
            "New password must be at least 8 characters".to_string(),
        ));
    }

    // Validate new password is not the trivially weak bootstrap password
    if req.new_password == "admin" {
        return Err(AppError::Validation(
            "New password cannot be the default password".to_string(),
        ));
    }

    // Fetch current user
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT id, password FROM users WHERE id = ?")
            .bind(&claims.sub)
            .fetch_optional(&state.db)
            .await?;

    let (user_id, password_hash) = row.ok_or(AppError::NotFound("User not found".to_string()))?;

    // Verify current password
    if !crate::auth::password::verify(&req.current_password, &password_hash) {
        return Err(AppError::Validation(
            "Current password is incorrect".to_string(),
        ));
    }

    // Hash new password
    let new_password_hash = crate::auth::password::hash(&req.new_password)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    // Update password
    let now_str = chrono::Utc::now().to_rfc3339();
    sqlx::query("UPDATE users SET password = $1, updated_at = $2 WHERE id = $3")
        .bind(&new_password_hash)
        .bind(&now_str)
        .bind(&user_id)
        .execute(&state.db)
        .await?;

    // Audit log: record password change
    crate::db::audit::log_action(
        state.db.clone(),
        user_id.clone(),
        claims.username.clone(),
        "password_change",
        "user",
        Some(user_id.clone()),
        None,
        ip,
    );

    tracing::info!("User {} changed their password", claims.username);

    Ok(Json(json!({
        "success": true,
        "message": "Password changed successfully"
    })))
}
