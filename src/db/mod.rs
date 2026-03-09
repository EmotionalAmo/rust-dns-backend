use crate::config::Config;
use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

pub mod app_catalog_cache;
pub mod audit;
pub mod models;
pub mod query_log_writer;

pub type DbPool = PgPool;

pub async fn init(cfg: &Config) -> Result<DbPool> {
    // PostgreSQL connection pool
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(20)
        .connect(&cfg.database.url)
        .await?;

    // Run migrations
    sqlx::migrate!("./src/db/migrations").run(&pool).await?;

    tracing::info!("Database connected to PostgreSQL");
    Ok(pool)
}

/// Create default admin user if no users exist yet.
pub async fn seed_admin(pool: &DbPool, _cfg: &Config) -> Result<()> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;

    if count.0 == 0 {
        let id = Uuid::new_v4().to_string();
        let password = crate::auth::password::hash("admin")?;

        sqlx::query(
            "INSERT INTO users (id, username, password, role, is_active, created_at, updated_at)
             VALUES ($1, $2, $3, 'super_admin', true, NOW(), NOW())",
        )
        .bind(&id)
        .bind("admin")
        .bind(&password)
        .execute(pool)
        .await?;

        tracing::warn!(
            "Created default admin user (username: admin, password: admin). \
             Change immediately in production!"
        );
    }

    Ok(())
}
