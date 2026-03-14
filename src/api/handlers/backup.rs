use crate::api::middleware::client_ip::ClientIp;
use crate::api::{middleware::rbac::AdminUser, AppState};
use crate::error::AppResult;
use axum::{body::Body, extract::State, http::StatusCode, response::Response};
use chrono::Utc;
use std::sync::Arc;

/// Create a PostgreSQL backup and return it as a binary download.
///
/// The response is a pg_dump custom-format file streamed directly to the caller.
/// No server-side storage is required — the client saves the file locally.
pub async fn create_backup(
    State(state): State<Arc<AppState>>,
    ClientIp(ip): ClientIp,
    admin: AdminUser,
) -> AppResult<Response<Body>> {
    let timestamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let filename = format!("rust-dns-backup-{}.dump", timestamp);

    let output = tokio::process::Command::new("pg_dump")
        .args(["--format=custom", "--no-password", &state.db_url])
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                crate::error::AppError::Internal(
                    "pg_dump not found. Ensure the Docker image includes postgresql-client."
                        .to_string(),
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

    let size = output.stdout.len();
    tracing::info!("Backup created: {} ({} bytes)", filename, size);

    crate::db::audit::log_action(
        state.db.clone(),
        admin.0.sub.clone(),
        admin.0.username.clone(),
        "create",
        "backup",
        None,
        Some(filename.clone()),
        ip,
    );

    let content_disposition = format!("attachment; filename=\"{}\"", filename);

    let response = Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/octet-stream")
        .header("content-disposition", content_disposition)
        .header("x-backup-size-bytes", size.to_string())
        .body(Body::from(output.stdout))
        .map_err(|e| crate::error::AppError::Internal(e.to_string()))?;

    Ok(response)
}
