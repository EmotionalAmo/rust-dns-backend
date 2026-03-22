use crate::config::Config;
use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

pub mod app_catalog_cache;
pub mod audit;
pub mod models;
pub mod query_log_writer;

pub type DbPool = PgPool;

/// Ensure the target database exists. Connects to the default `postgres`
/// maintenance database, and runs `CREATE DATABASE` if needed.
async fn ensure_database_exists(database_url: &str) -> Result<()> {
    let mut parsed = url::Url::parse(database_url)?;

    // Extract the database name from the path (e.g. "/rust_dns" → "rust_dns")
    let db_name = parsed.path().trim_start_matches('/').to_owned();

    if db_name.is_empty() {
        anyhow::bail!("Database URL has no database name in path");
    }

    // Switch to the maintenance database
    parsed.set_path("/postgres");
    let maintenance_url = parsed.to_string();

    let maintenance = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&maintenance_url)
        .await?;

    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = $1)")
            .bind(&db_name)
            .fetch_one(&maintenance)
            .await?;

    if !exists {
        tracing::info!("Database '{}' not found, creating it…", db_name);
        // CREATE DATABASE cannot run inside a transaction; use raw execute.
        sqlx::query(&format!("CREATE DATABASE \"{}\"", db_name))
            .execute(&maintenance)
            .await?;
        tracing::info!("Database '{}' created", db_name);
    }

    maintenance.close().await;
    Ok(())
}

pub async fn init(cfg: &Config) -> Result<DbPool> {
    // Ensure the target database exists before connecting to it
    ensure_database_exists(&cfg.database.url).await?;

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
