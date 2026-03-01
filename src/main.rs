use anyhow::Result;
use chrono::Utc;
use clap::Parser;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{error, info, warn};
use tracing_appender::non_blocking::WorkerGuard;

// Re-use modules from the library crate
use rust_dns::api;
use rust_dns::config;
use rust_dns::db;
use rust_dns::dns;
use rust_dns::metrics;
use rust_dns::shutdown;

#[derive(Parser, Debug)]
#[command(
    name = "ent-dns",
    version,
    about = "Enterprise DNS filtering server",
    long_about = "Ent-DNS: high-performance DNS filtering proxy with WebUI.\n\
                  Config is loaded in this priority order:\n\
                  1. Environment variables (ENT_DNS__<SECTION>__<KEY>)\n\
                  2. --config file or ENT_DNS_CONFIG env var\n\
                  3. ./config.toml or /etc/ent-dns/config.toml (auto-discovered)\n\
                  4. Built-in defaults"
)]
struct Args {
    /// Path to TOML config file.
    /// Also readable from ENT_DNS_CONFIG environment variable.
    #[arg(short, long, env = "ENT_DNS_CONFIG", value_name = "FILE")]
    config: Option<String>,
}

/// Initialise the tracing subscriber according to `LoggingConfig`.
///
/// Returns an `Option<WorkerGuard>` that **must** be kept alive for the entire
/// duration of `main()`.  Dropping the guard flushes and closes the log file.
fn init_logging(logging: &config::LoggingConfig) -> Result<Option<WorkerGuard>> {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

    // RUST_LOG takes priority; fall back to config value.
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&logging.level));

    // Build an optional file layer and capture the non-blocking guard.
    // Using Option<Layer> works because Option<L: Layer<S>> also implements Layer<S>.
    let mut guard: Option<WorkerGuard> = None;

    let file_layer = if let Some(ref log_file_path) = logging.file {
        let path = std::path::Path::new(log_file_path);
        let dir = path.parent().unwrap_or(std::path::Path::new("."));
        let file_name = path
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("ent-dns.log"));

        if !dir.exists() {
            std::fs::create_dir_all(dir).map_err(|e| {
                anyhow::anyhow!("Cannot create log directory {}: {}", dir.display(), e)
            })?;
        }

        let appender = match logging.rotation.as_str() {
            "hourly" => tracing_appender::rolling::hourly(dir, file_name),
            "never" => tracing_appender::rolling::never(dir, file_name),
            _ => tracing_appender::rolling::daily(dir, file_name),
        };
        let (non_blocking, g) = tracing_appender::non_blocking(appender);
        guard = Some(g);

        Some(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false),
        )
    } else {
        None
    };

    // Console layer: enabled when there is no file OR when console=true alongside a file.
    let console_layer = if logging.file.is_none() || logging.console {
        Some(tracing_subscriber::fmt::layer())
    } else {
        None
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(console_layer)
        .with(file_layer)
        .init();

    Ok(guard)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Config must be loaded before logging so we can honour the log level from
    // the config file.  We use a temporary stderr logger until the real one is
    // ready.
    let cfg = config::load(args.config.as_deref())?;

    // Initialise structured logging.  The guard must stay alive until main()
    // returns so the background file-writer thread has time to flush.
    let _log_guard = init_logging(&cfg.logging)?;

    info!("Starting Ent-DNS Enterprise v{}", env!("CARGO_PKG_VERSION"));
    info!("Configuration loaded");

    let db_pool = db::init(&cfg).await?;
    info!("Database initialized");

    // Seed initial admin user if none exist
    db::seed_admin(&db_pool, &cfg).await?;

    // Shared DNS metrics between DNS server and API
    let metrics = Arc::new(metrics::DnsMetrics::default());

    // In-memory App Catalog cache (Optimization for DNS query log insertion)
    let app_catalog = Arc::new(db::app_catalog_cache::AppCatalogCache::new());
    app_catalog.load_from_db(&db_pool).await;

    // FilterEngine shared between DNS engine and Management API
    let filter = Arc::new(dns::filter::FilterEngine::new(db_pool.clone()).await?);

    // Broadcast channel for real-time query log push (WebSocket)
    let (query_log_tx, _) = broadcast::channel::<serde_json::Value>(256);

    // Graceful shutdown signal
    let shutdown_signal = shutdown::ShutdownSignal::new();

    // Background: auto-refresh filter lists based on each list's update_interval_hours
    {
        let db = db_pool.clone();
        let filter_engine = filter.clone();
        let mut shutdown_rx = shutdown_signal.subscribe();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(tokio::time::Duration::from_secs(3600));
            ticker.tick().await; // skip immediate first tick
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        tracing::info!("Auto-refresh: checking filter lists...");

                        let lists: Vec<(String, String, Option<i64>, Option<String>)> =
                            match sqlx::query_as(
                                "SELECT id, url, update_interval_hours, last_updated
                             FROM filter_lists WHERE is_enabled = 1 AND url != '' AND url IS NOT NULL",
                            )
                            .fetch_all(&db)
                            .await
                            {
                                Ok(r) => r,
                                Err(e) => {
                                    tracing::warn!("Auto-refresh DB error: {}", e);
                                    continue;
                                }
                            };

                        let mut refreshed = false;
                        for (id, url, interval_hours, last_updated) in lists {
                            let interval_h = interval_hours.unwrap_or(0);
                            // 0 = manual only, skip auto-refresh
                            if interval_h <= 0 {
                                continue;
                            }
                            let due = last_updated
                                .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                                .map(|last| {
                                    let elapsed =
                                        Utc::now().signed_duration_since(last.with_timezone(&Utc));
                                    elapsed.num_hours() >= interval_h
                                })
                                .unwrap_or(true);

                            if due {
                                match dns::subscription::sync_filter_list(&db, &id, &url).await {
                                    Ok(n) => {
                                        tracing::info!("Auto-refreshed filter {}: {} rules", id, n);
                                        refreshed = true;
                                    }
                                    Err(e) => tracing::warn!("Auto-refresh filter {}: {}", id, e),
                                }
                            }
                        }

                        if refreshed {
                            if let Err(e) = filter_engine.reload().await {
                                tracing::warn!("Filter reload after auto-refresh: {}", e);
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        tracing::info!("Filter auto-refresh task shutting down");
                        break;
                    }
                }
            }
        });
    }

    // Background: auto-cleanup query log based on query_log_retention_days setting
    // Rotates logs daily to prevent database from growing indefinitely
    {
        let db = db_pool.clone();
        let cfg_clone = cfg.clone();
        let mut shutdown_rx = shutdown_signal.subscribe();
        tokio::spawn(async move {
            let retention_days = cfg_clone.database.query_log_retention_days;

            // Run daily at 3 AM (24h interval)
            let mut ticker = tokio::time::interval(tokio::time::Duration::from_secs(86400));

            tracing::info!(
                "Query log rotation enabled: retaining {} days, running daily",
                retention_days
            );

            ticker.tick().await; // skip immediate first tick
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        match sqlx::query(
                            "DELETE FROM query_log WHERE time < datetime('now', '-' || ? || ' days')",
                        )
                        .bind(retention_days as i64)
                        .execute(&db)
                        .await
                        {
                            Ok(r) if r.rows_affected() > 0 => tracing::info!(
                                "Query log rotation: deleted {} entries older than {} days",
                                r.rows_affected(),
                                retention_days
                            ),
                            Ok(_) => {}
                            Err(e) => tracing::warn!("Query log rotation error: {}", e),
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        tracing::info!("Query log rotation task shutting down");
                        break;
                    }
                }
            }
        });
    }

    // Build a single DnsHandler shared between the DNS server (UDP/TCP) and the
    // API server (DoH endpoint). Both use the same filter, cache, and log writer.
    let dns_handler = dns::build_handler(
        &cfg,
        db_pool.clone(),
        filter.clone(),
        metrics.clone(),
        query_log_tx.clone(),
        app_catalog.clone(),
    )
    .await?;

    // Spawn DNS and API servers
    let dns_shutdown_signal = shutdown_signal.clone();
    let dns_handler_clone = dns_handler.clone();
    let dns_cfg = cfg.clone();
    let dns_task = tokio::spawn(async move {
        if let Err(e) = dns::serve(dns_handler_clone, &dns_cfg, dns_shutdown_signal).await {
            error!("DNS server error: {}", e);
        }
    });

    let api_shutdown_signal = shutdown_signal.clone();
    let api_cfg = cfg.clone();
    let api_db = db_pool.clone();
    let api_filter = filter.clone();
    let api_metrics = metrics.clone();
    let api_query_log_tx = query_log_tx;
    let api_dns_handler = dns_handler;
    let api_task = tokio::spawn(async move {
        if let Err(e) = api::serve(
            api_cfg,
            api_db,
            api_filter,
            api_metrics,
            api_query_log_tx,
            api_dns_handler,
            api_shutdown_signal,
        )
        .await
        {
            error!("API server error: {}", e);
        }
    });

    // Wait for termination signal
    shutdown::wait_for_termination_signal().await;

    info!("Graceful shutdown initiated...");

    // Broadcast shutdown to all tasks
    shutdown_signal.initiate();

    // Wait for DNS and API servers with timeout
    let shutdown_timeout = shutdown::DEFAULT_SHUTDOWN_TIMEOUT_SECS;
    tokio::select! {
        _ = tokio::time::sleep(tokio::time::Duration::from_secs(shutdown_timeout)) => {
            warn!("Graceful shutdown timeout ({}s), forcing exit", shutdown_timeout);
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Received Ctrl-C during shutdown, forcing exit");
        }
        result = async { tokio::try_join!(dns_task, api_task) } => {
            match result {
                Ok(_) => info!("All servers shut down gracefully"),
                Err(e) => error!("Shutdown error: {:?}", e),
            }
        }
    }

    // Safe SQLite shutdown: WAL checkpoint before closing
    info!("Performing SQLite WAL checkpoint...");
    match sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(&db_pool)
        .await
    {
        Ok(_) => info!("SQLite WAL checkpoint completed"),
        Err(e) => warn!("SQLite WAL checkpoint failed: {}", e),
    }

    // Close database pool
    info!("Closing database connection pool...");
    db_pool.close().await;

    info!("Ent-DNS shutdown complete");
    Ok(())
}
