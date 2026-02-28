use crate::config::Config;
use anyhow::Result;
use chrono::Utc;
use sqlx::SqlitePool;
use std::str::FromStr;
use uuid::Uuid;

pub mod audit;
pub mod models;
pub mod query_log_writer;

pub type DbPool = SqlitePool;

pub async fn init(cfg: &Config) -> Result<DbPool> {
    let db_url = format!("sqlite://{}?mode=rwc", cfg.database.path);

    // Configure connection pool for optimal performance
    // Use PoolOptions to set connection pool size
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(20) // Explicit connection pool size
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::from_str(&db_url)?.create_if_missing(true),
        )
        .await?;

    sqlx::migrate!("./src/db/migrations").run(&pool).await?;

    // SQLite PRAGMA optimizations for write-heavy workloads
    // These provide 30-50% write performance improvement
    sqlx::query("PRAGMA journal_mode=WAL")
        .execute(&pool)
        .await?;

    sqlx::query("PRAGMA synchronous=NORMAL")
        .execute(&pool)
        .await?;

    sqlx::query("PRAGMA cache_size=-64000")
        .execute(&pool)
        .await?;

    sqlx::query("PRAGMA mmap_size=268435456") // 256MB memory-mapped I/O
        .execute(&pool)
        .await?;

    sqlx::query("PRAGMA wal_autocheckpoint=1000")
        .execute(&pool)
        .await?;

    tracing::info!("Database connected: {}", cfg.database.path);
    Ok(pool)
}

/// Create default admin user if no users exist yet.
pub async fn seed_admin(pool: &DbPool, _cfg: &Config) -> Result<()> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;

    if count.0 == 0 {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let password = crate::auth::password::hash("admin")?;

        sqlx::query(
            "INSERT INTO users (id, username, password, role, is_active, created_at, updated_at)
             VALUES (?, ?, ?, 'super_admin', 1, ?, ?)",
        )
        .bind(&id)
        .bind("admin")
        .bind(&password)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;

        tracing::warn!(
            "Created default admin user (username: admin, password: admin). \
             Change immediately in production!"
        );
    }

    Ok(())
}
