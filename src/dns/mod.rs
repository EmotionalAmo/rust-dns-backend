use crate::config::Config;
use crate::db::DbPool;
use crate::metrics::DnsMetrics;
use crate::shutdown::ShutdownSignal;
use anyhow::Result;
use filter::FilterEngine;
use std::sync::Arc;
use tokio::sync::broadcast;

pub mod acl;
pub mod cache;
pub mod filter;
pub mod handler;
pub mod resolver;
pub mod rules;
pub mod server;
pub mod subscription;
pub mod upstream_pool;

pub use handler::DnsHandler;

/// Build a shared `DnsHandler`.  Call this once in `main`, then pass the Arc
/// both to `serve` (for UDP/TCP DNS) and to `AppState` (for the DoH HTTP endpoint).
pub async fn build_handler(
    cfg: &Config,
    db: DbPool,
    filter: Arc<FilterEngine>,
    metrics: Arc<DnsMetrics>,
    query_log_tx: broadcast::Sender<serde_json::Value>,
    app_catalog: Arc<crate::db::app_catalog_cache::AppCatalogCache>,
) -> Result<Arc<DnsHandler>> {
    Ok(Arc::new(
        DnsHandler::new(cfg.clone(), db, filter, metrics, query_log_tx, app_catalog).await?,
    ))
}

/// Start the DNS server (UDP + TCP) using a previously built handler.
/// This function blocks until the server is shut down via the shutdown signal.
pub async fn serve(
    handler: Arc<DnsHandler>,
    cfg: &Config,
    shutdown_signal: ShutdownSignal,
) -> Result<()> {
    let bind_addr = format!("{}:{}", cfg.dns.bind, cfg.dns.port);
    tracing::info!("DNS server starting on {}", bind_addr);
    server::run(handler, bind_addr, shutdown_signal).await
}
