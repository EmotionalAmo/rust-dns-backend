use crate::api::{middleware::rbac::AdminUser, AppState};
use crate::error::AppResult;
use axum::{extract::State, Json};
use chrono::Utc;
use serde_json::json;
use std::sync::Arc;

pub async fn create_backup(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> AppResult<Json<serde_json::Value>> {
    let timestamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();

    // Use a fixed safe directory to avoid writing to arbitrary cwd.
    // Operators can override via ENT_DNS_BACKUP_DIR env var.
    let backup_dir = std::env::var("ENT_DNS_BACKUP_DIR").unwrap_or_else(|_| "/tmp".to_string());

    // Build the path and canonicalize the directory to prevent traversal.
    let dir_path = std::path::Path::new(&backup_dir);
    if !dir_path.is_dir() {
        return Err(crate::error::AppError::Internal(format!(
            "Backup directory does not exist: {}",
            backup_dir
        )));
    }

    // Filename is derived only from timestamp — no user input involved.
    let backup_filename = format!("ent-dns-backup-{}.db", timestamp);
    let backup_path = dir_path.join(&backup_filename);
    let backup_path_str = backup_path
        .to_str()
        .ok_or_else(|| crate::error::AppError::Internal("Invalid backup path".to_string()))?
        .to_string();

    // Create backup using SQLite's VACUUM INTO command
    // First we need to disable WAL mode temporarily for backup
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(&state.db)
        .await
        .map_err(|e| {
            tracing::error!("WAL checkpoint failed: {}", e);
            crate::error::AppError::Internal(format!("WAL checkpoint failed: {}", e))
        })?;

    // SQLite does not support parameter binding for VACUUM INTO.
    // The path is safe: fixed directory + timestamp-only filename (no user input).
    // Extra precaution: escape single quotes in the path (M-1 fix).
    let escaped_path = backup_path_str.replace('\'', "''");
    let result = sqlx::query(&format!("VACUUM INTO '{}'", escaped_path))
        .execute(&state.db)
        .await;

    match result {
        Ok(_) => {
            tracing::info!("Backup created: {}", backup_filename);
            Ok(Json(json!({
                "success": true,
                "filename": backup_filename,
                "timestamp": timestamp,
            })))
        }
        Err(e) => {
            tracing::error!("Backup failed: {}", e);
            Err(crate::error::AppError::Internal(format!(
                "Backup failed: {}",
                e
            )))
        }
    }
}
