use crate::api::validators::rule::RuleValidationResponse;
use crate::config::Config;
use crate::db::models::client_group::DnsRuleWithSource;
use crate::db::DbPool;
use crate::dns::filter::FilterEngine;
use crate::dns::DnsHandler;
use crate::metrics::DnsMetrics;
use crate::shutdown::ShutdownSignal;
use anyhow::Result;
use axum::http::{header, HeaderValue, Method};
use axum::Router;
use dashmap::DashMap;
use moka::future::Cache;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

pub mod handlers;
pub mod middleware;
pub mod router;
pub mod validators;

#[derive(Debug, Clone, serde::Serialize)]
pub struct UpstreamHealthResult {
    pub status: String,
    pub latency_ms: i64,
    pub last_check_at: String,
}

pub struct AppState {
    pub db: DbPool,
    pub filter: Arc<FilterEngine>,
    pub jwt_secret: String,
    pub jwt_expiry_hours: u64,
    pub metrics: Arc<DnsMetrics>,
    pub query_log_tx: broadcast::Sender<serde_json::Value>,
    /// One-time WebSocket tickets: ticket_uuid → issued_at (H-2)
    pub ws_tickets: DashMap<String, Instant>,
    /// Login failure tracking: ip → (failure_count, window_start) (H-5)
    pub login_attempts: DashMap<String, (u32, Instant)>,
    /// Shared DNS handler — used by the DoH endpoint (Task 5)
    pub dns_handler: Arc<DnsHandler>,
    /// Rule validation cache: (type + rule) → validation result
    pub rule_validation_cache: Arc<Cache<String, RuleValidationResponse>>,
    /// Client configuration cache: client_id → Vec<DnsRuleWithSource> (Task 12)
    pub client_config_cache: Option<Arc<Cache<String, Vec<DnsRuleWithSource>>>>,
    /// Frontend static files directory (from config api.static_dir)
    pub static_dir: String,
    /// Allow using the default password without forcing a change (for testing)
    pub allow_default_password: bool,
    /// Upstream health check results (H-7)
    pub upstream_health: DashMap<String, UpstreamHealthResult>,
    /// Suggest query cache: "field:prefix:limit" → Vec<String>, 60s TTL
    pub suggest_cache: Arc<Cache<String, Vec<String>>>,
    /// Blacklisted JWT IDs: jti → (). Populated on logout; entries survive until token expiry.
    /// In-memory only — restarts clear the blacklist, but tokens expire within jwt_expiry_hours anyway.
    pub token_blacklist: dashmap::DashMap<String, ()>,
}

/// Start the API server.
///
/// This function blocks until the server is shut down or an error occurs.
/// Use the provided `shutdown_signal` to trigger graceful shutdown.
pub async fn serve(
    cfg: Config,
    db: DbPool,
    filter: Arc<FilterEngine>,
    metrics: Arc<DnsMetrics>,
    query_log_tx: broadcast::Sender<serde_json::Value>,
    dns_handler: Arc<DnsHandler>,
    shutdown_signal: ShutdownSignal,
) -> Result<()> {
    let bind_addr = format!("{}:{}", cfg.api.bind, cfg.api.port);

    // Rule validation cache: 1000 entries, 5 minutes TTL
    let rule_validation_cache = Arc::new(
        Cache::builder()
            .max_capacity(1000)
            .time_to_live(std::time::Duration::from_secs(300))
            .build(),
    );

    // Client configuration cache: 4096 entries, 60 seconds TTL (Task 12)
    let client_config_cache = Arc::new(
        Cache::builder()
            .max_capacity(4096)
            .time_to_live(std::time::Duration::from_secs(60))
            .build(),
    );

    // Suggest query cache: 1000 entries, 60 seconds TTL
    let suggest_cache: Arc<Cache<String, Vec<String>>> = Arc::new(
        Cache::builder()
            .max_capacity(1000)
            .time_to_live(std::time::Duration::from_secs(60))
            .build(),
    );

    let db_for_task = db.clone();
    let state = Arc::new(AppState {
        db,
        filter,
        jwt_secret: cfg.auth.jwt_secret.clone(),
        jwt_expiry_hours: cfg.auth.jwt_expiry_hours,
        metrics,
        query_log_tx,
        ws_tickets: DashMap::new(),
        login_attempts: DashMap::new(),
        dns_handler: dns_handler.clone(),
        rule_validation_cache,
        client_config_cache: Some(client_config_cache),
        static_dir: cfg.api.static_dir.clone(),
        allow_default_password: cfg.auth.allow_default_password,
        upstream_health: DashMap::new(),
        suggest_cache,
        token_blacklist: DashMap::new(),
    });
    let cors = build_cors_layer(&cfg.api.cors_allowed_origins);
    let app = build_app(state.clone(), cors);

    // Use into_make_service_with_connect_info to expose the real TCP peer IP (H-3)
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    tracing::info!("Management API listening on http://{}", bind_addr);

    // Wait for shutdown signal
    let mut shutdown_rx = shutdown_signal.subscribe();

    // Background: upstream health check (runs every 30s, checks each upstream per its own interval)
    {
        let db = db_for_task;
        let mut shutdown_rx_task = shutdown_signal.subscribe();
        let health_map = state.upstream_health.clone();
        let dns_handler = dns_handler.clone();
        tokio::spawn(async move {
            // Initial delay to let the server finish starting up
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
            let mut ticker = tokio::time::interval(tokio::time::Duration::from_secs(30));
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let rows: Vec<(String, String, i64, i64, Option<String>)> = match sqlx::query_as(
                            "SELECT id, addresses, health_check_interval, health_check_timeout, last_health_check_at
                             FROM dns_upstreams WHERE is_active = 1 AND health_check_enabled = 1"
                        ).fetch_all(&db).await {
                            Ok(r) => r,
                            Err(e) => { tracing::warn!("Upstream health check DB error: {}", e); continue; }
                        };

                        for (id, addresses, interval_secs, timeout_secs, last_check_at) in rows {
                            let due = last_check_at
                                .as_deref()
                                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                                .map(|last| {
                                    let elapsed = chrono::Utc::now()
                                        .signed_duration_since(last.with_timezone(&chrono::Utc));
                                    elapsed.num_seconds() >= interval_secs
                                })
                                .unwrap_or(true);

                            if !due {
                                continue;
                            }

                            let addresses_vec: Vec<String> = match serde_json::from_str(&addresses) {
                                Ok(v) => v,
                                Err(_) => continue,
                            };
                            let first_addr = match addresses_vec.first() {
                                Some(a) => a.clone(),
                                None => continue,
                            };
                            let timeout = std::time::Duration::from_secs(timeout_secs as u64);
                            let start = std::time::Instant::now();
                            let success = check_upstream_connectivity(&first_addr, timeout)
                                .await
                                .is_ok();
                            let latency_ms = start.elapsed().as_millis() as i64;
                            let now = chrono::Utc::now().to_rfc3339();
                            let new_status = if success { "healthy" } else { "degraded" };

                            // Update in-memory cache
                            health_map.insert(id.clone(), UpstreamHealthResult {
                                status: new_status.to_string(),
                                latency_ms,
                                last_check_at: now.clone(),
                            });

                            // Inject latency into the running DNS handler for the Fastest strategy
                            dns_handler.update_upstream_latency(&id, latency_ms).await;

                            let _ = sqlx::query(
                                "INSERT INTO upstream_latency_log (upstream_id, latency_ms, success, checked_at) VALUES ($1, $2, $3, $4)"
                            )
                            .bind(&id)
                            .bind(latency_ms)
                            .bind(if success { 1i64 } else { 0i64 })
                            .bind(&now)
                            .execute(&db)
                            .await;

                            let _ = sqlx::query(
                                "UPDATE dns_upstreams SET health_status = $1, last_health_check_at = $2, updated_at = $3 WHERE id = $4"
                            )
                            .bind(new_status)
                            .bind(&now)
                            .bind(&now)
                            .bind(&id)
                            .execute(&db)
                            .await;

                            tracing::debug!(
                                "Upstream health check {}: {} ({}ms)",
                                id,
                                new_status,
                                latency_ms
                            );
                        }

                        let _ = sqlx::query(
                            "DELETE FROM upstream_latency_log WHERE checked_at < NOW() - INTERVAL '1 day'",
                        )
                        .execute(&db)
                        .await;
                    }
                    _ = shutdown_rx_task.recv() => {
                        tracing::info!("Upstream health check task shutting down");
                        break;
                    }
                }
            }
        });
    }

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown_rx.recv().await.ok();
        tracing::info!("API server graceful shutdown initiated");
    })
    .await?;

    Ok(())
}

/// Minimal DNS connectivity check used by the background health monitor.
/// Supports UDP (plain IP), DoH (https://), and DoT (tls://) upstreams.
async fn check_upstream_connectivity(
    addr: &str,
    timeout: std::time::Duration,
) -> anyhow::Result<()> {
    use hickory_resolver::config::{
        NameServerConfig, NameServerConfigGroup, Protocol, ResolverConfig, ResolverOpts,
    };
    use hickory_resolver::TokioAsyncResolver;
    use std::net::ToSocketAddrs;

    let config = if addr.starts_with("https://") {
        // DoH upstream health check
        let (host, port) = parse_url_host_port(addr, "https://", 443)?;
        let lookup_target = format!("{}:{}", host, port);
        let addrs: Vec<std::net::IpAddr> = lookup_target
            .as_str()
            .to_socket_addrs()
            .map_err(|e| anyhow::anyhow!("Failed to resolve DoH host '{}': {}", host, e))?
            .map(|a| a.ip())
            .collect();
        anyhow::ensure!(
            !addrs.is_empty(),
            "DoH host '{}' resolved to no addresses",
            host
        );
        let ns_group = NameServerConfigGroup::from_ips_https(&addrs, port, host, false);
        let mut cfg = ResolverConfig::new();
        for ns in ns_group.into_inner() {
            cfg.add_name_server(ns);
        }
        cfg
    } else if addr.starts_with("tls://") {
        // DoT upstream health check — performs a real TLS handshake via hickory
        let (host, port) = parse_url_host_port(addr, "tls://", 853)?;
        let lookup_target = format!("{}:{}", host, port);
        let addrs: Vec<std::net::IpAddr> = lookup_target
            .as_str()
            .to_socket_addrs()
            .map_err(|e| anyhow::anyhow!("Failed to resolve DoT host '{}': {}", host, e))?
            .map(|a| a.ip())
            .collect();
        anyhow::ensure!(
            !addrs.is_empty(),
            "DoT host '{}' resolved to no addresses",
            host
        );
        let ns_group = NameServerConfigGroup::from_ips_tls(&addrs, port, host, false);
        let mut cfg = ResolverConfig::new();
        for ns in ns_group.into_inner() {
            cfg.add_name_server(ns);
        }
        cfg
    } else {
        // Plain UDP upstream health check
        let (ip_str, port) = if addr.contains(':') {
            let parts: Vec<&str> = addr.rsplitn(2, ':').collect();
            (parts[1], parts[0].parse::<u16>()?)
        } else {
            (addr, 53u16)
        };
        let ip_addr = ip_str.parse::<std::net::IpAddr>()?;
        let mut cfg = ResolverConfig::new();
        cfg.add_name_server(NameServerConfig {
            socket_addr: (ip_addr, port).into(),
            protocol: Protocol::Udp,
            trust_negative_responses: false,
            tls_config: None,
            bind_addr: None,
            tls_dns_name: None,
        });
        cfg
    };

    let resolver = TokioAsyncResolver::tokio(config, ResolverOpts::default());
    let _ = tokio::time::timeout(timeout, resolver.lookup_ip("example.com.")).await??;
    Ok(())
}

/// Parse (host, port) from a URL with the given scheme prefix and default port.
/// Examples: "https://dns.cloudflare.com/dns-query" → ("dns.cloudflare.com", 443)
///           "tls://1.1.1.1" → ("1.1.1.1", 853)
fn parse_url_host_port(
    url: &str,
    prefix: &str,
    default_port: u16,
) -> anyhow::Result<(String, u16)> {
    let rest = url
        .strip_prefix(prefix)
        .ok_or_else(|| anyhow::anyhow!("URL must start with {}: {}", prefix, url))?;
    // Strip any path component
    let authority = match rest.find('/') {
        Some(idx) => &rest[..idx],
        None => rest,
    };
    if authority.is_empty() {
        anyhow::bail!("Empty host in URL: {}", url);
    }
    // Handle IPv6 literals like [::1] or [::1]:853
    if authority.starts_with('[') {
        let end = authority
            .rfind(']')
            .ok_or_else(|| anyhow::anyhow!("Malformed IPv6 address in URL: {}", url))?;
        let host = authority[1..end].to_string();
        let port_part = &authority[end + 1..];
        let port = if port_part.is_empty() {
            default_port
        } else {
            port_part
                .strip_prefix(':')
                .and_then(|p| p.parse::<u16>().ok())
                .ok_or_else(|| anyhow::anyhow!("Invalid port in URL: {}", url))?
        };
        return Ok((host, port));
    }
    // Regular hostname or IPv4
    match authority.rfind(':') {
        Some(idx) => {
            let host = authority[..idx].to_string();
            let port = authority[idx + 1..]
                .parse::<u16>()
                .map_err(|_| anyhow::anyhow!("Invalid port in URL: {}", url))?;
            Ok((host, port))
        }
        None => Ok((authority.to_string(), default_port)),
    }
}

fn build_cors_layer(allowed_origins: &[String]) -> CorsLayer {
    let origins: Vec<HeaderValue> = allowed_origins
        .iter()
        .filter_map(|o| o.parse().ok())
        .collect();

    if origins.is_empty() {
        tracing::warn!(
            "No valid CORS origins configured; CORS will block all cross-origin requests"
        );
        return CorsLayer::new();
    }

    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
}

pub fn build_app(state: Arc<AppState>, cors: CorsLayer) -> Router {
    Router::new()
        .merge(router::routes(state.clone()))
        .layer(axum::middleware::from_fn_with_state(
            state,
            middleware::audit::audit_middleware,
        ))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        // Security headers — applied to every HTTP response
        .layer(SetResponseHeaderLayer::if_not_present(
            header::HeaderName::from_static("x-frame-options"),
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::HeaderName::from_static("x-xss-protection"),
            HeaderValue::from_static("1; mode=block"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::HeaderName::from_static("referrer-policy"),
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        ))
}
