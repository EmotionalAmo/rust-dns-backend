use crate::db::DbPool;
use chrono::Utc;

/// Fire-and-forget: write an audit log entry to the database.
/// Spawns a background task so the caller is never blocked.
#[allow(clippy::too_many_arguments)]
pub fn log_action(
    db: DbPool,
    user_id: String,
    username: String,
    action: impl Into<String> + Send + 'static,
    resource: impl Into<String> + Send + 'static,
    resource_id: Option<String>,
    detail: Option<String>,
    ip: String,
) {
    let action = action.into();
    let resource = resource.into();
    let now = Utc::now().to_rfc3339();

    tokio::spawn(async move {
        let _ = sqlx::query(
            "INSERT INTO audit_log (time, user_id, username, action, resource, resource_id, detail, ip)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"
        )
        .bind(&now)
        .bind(&user_id)
        .bind(&username)
        .bind(&action)
        .bind(&resource)
        .bind(resource_id.as_deref())
        .bind(detail.as_deref())
        .bind(&ip)
        .execute(&db)
        .await;
    });
}
