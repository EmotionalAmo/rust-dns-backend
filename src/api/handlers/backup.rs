use crate::api::middleware::client_ip::ClientIp;
use crate::api::{middleware::rbac::AdminUser, AppState};
use crate::error::AppResult;
use axum::{extract::State, Json};
use chrono::Utc;
use serde_json::json;
use std::sync::Arc;

pub async fn create_backup(
    State(state): State<Arc<AppState>>,
    ClientIp(ip): ClientIp,
    admin: AdminUser,
) -> AppResult<Json<serde_json::Value>> {
    let timestamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();

    // Use a fixed safe directory to avoid writing to arbitrary cwd.
    // Operators can override via RUST_DNS_BACKUP_DIR env var.
    let backup_dir = std::env::var("RUST_DNS_BACKUP_DIR").unwrap_or_else(|_| "/tmp".to_string());

    // Build the path and canonicalize the directory to prevent traversal.
    let dir_path = std::path::Path::new(&backup_dir);
    if !dir_path.is_dir() {
        return Err(crate::error::AppError::Internal(format!(
            "Backup directory does not exist: {}",
            backup_dir
        )));
    }

    // Filename is derived only from timestamp — no user input involved.
    let backup_filename = format!("rust-dns-backup-{}.dump", timestamp);
    let backup_path = dir_path.join(&backup_filename);
    let backup_path_str = backup_path
        .to_str()
        .ok_or_else(|| crate::error::AppError::Internal("Invalid backup path".to_string()))?
        .to_string();

    // Run pg_dump --format=custom to create a PostgreSQL binary backup.
    // pg_dump is part of the standard PostgreSQL client tools.
    let output = tokio::process::Command::new("pg_dump")
        .args([
            "--format=custom",
            "--no-password",
            "--file",
            &backup_path_str,
            &state.db_url,
        ])
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                crate::error::AppError::Internal(
                    "pg_dump not found. Please install postgresql-client on the host.".to_string(),
                )
            } else {
                crate::error::AppError::Internal(format!("Failed to run pg_dump: {}", e))
            }
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::error!("pg_dump failed: {}", stderr);
        return Err(crate::error::AppError::Internal(format!(
            "pg_dump failed: {}",
            stderr.trim()
        )));
    }

    // Get file size for the response.
    let file_size = tokio::fs::metadata(&backup_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    tracing::info!("Backup created: {} ({} bytes)", backup_filename, file_size);
    crate::db::audit::log_action(
        state.db.clone(),
        admin.0.sub.clone(),
        admin.0.username.clone(),
        "create",
        "backup",
        None,
        Some(backup_filename.clone()),
        ip,
    );

    Ok(Json(json!({
        "success": true,
        "filename": backup_filename,
        "timestamp": timestamp,
        "size_bytes": file_size,
    })))
}
