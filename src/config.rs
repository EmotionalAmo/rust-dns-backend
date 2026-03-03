use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub dns: DnsConfig,
    pub api: ApiConfig,
    pub database: DatabaseConfig,
    pub auth: AuthConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
}

/// Logging configuration — all fields have sane defaults so the section
/// can be omitted entirely from config.toml.
#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    /// Log level filter applied to rust_dns and its dependencies.
    /// Accepts the standard tracing level strings:
    ///   trace | debug | info | warn | error
    /// The RUST_LOG environment variable, when set, takes priority over this field.
    #[serde(default = "default_log_level")]
    pub level: String,

    /// Optional path for log file output.
    /// When set, log records are written to a rolling file at this path.
    /// Example: "/var/log/rust-dns/rust-dns.log"
    /// When absent, only console (stdout) output is produced.
    pub file: Option<String>,

    /// Rolling strategy for the log file: "daily" | "hourly" | "never"
    /// Has no effect when `file` is not set.
    #[serde(default = "default_log_rotation")]
    pub rotation: String,

    /// When true (default) and `file` is set, log records are written to
    /// both the file and stdout simultaneously.
    #[serde(default = "default_log_console")]
    pub console: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            file: None,
            rotation: default_log_rotation(),
            console: default_log_console(),
        }
    }
}

/// Default config file search paths (tried in order when no explicit path given).
const DEFAULT_CONFIG_PATHS: &[&str] = &["./config.toml", "/etc/rust-dns/config.toml"];

#[derive(Debug, Clone, Deserialize)]
pub struct DnsConfig {
    #[serde(default = "default_dns_port")]
    pub port: u16,
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default)]
    pub upstreams: Vec<String>,
    #[serde(default = "default_prefer_ipv4")]
    pub prefer_ipv4: bool,
    #[allow(dead_code)]
    pub doh_enabled: bool,
    #[allow(dead_code)]
    pub dot_enabled: bool,
    /// TTL（秒）用于 DNS 重写（rewrite）响应。默认 300 秒。
    /// 可通过 RUST_DNS__DNS__REWRITE_TTL 或 config.toml 中 dns.rewrite_ttl 配置。
    #[serde(default = "default_rewrite_ttl")]
    pub rewrite_ttl: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiConfig {
    #[serde(default = "default_api_port")]
    pub port: u16,
    #[serde(default = "default_bind")]
    pub bind: String,
    /// Allowed CORS origins. Defaults to localhost dev ports.
    /// Set RUST_DNS__API__CORS_ALLOWED_ORIGINS in production.
    #[serde(default = "default_cors_allowed_origins")]
    pub cors_allowed_origins: Vec<String>,
    /// Directory for frontend static files. Defaults to "frontend/dist".
    /// Override with RUST_DNS__API__STATIC_DIR in production.
    #[serde(default = "default_static_dir")]
    pub static_dir: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    #[serde(default = "default_db_path")]
    pub path: String,
    #[serde(default = "default_query_log_retention_days")]
    pub query_log_retention_days: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthConfig {
    pub jwt_secret: String,
    #[serde(default = "default_jwt_expiry")]
    pub jwt_expiry_hours: u64,
    /// Allow using the default password without forcing a change.
    /// Intended for testing/CI environments only.
    #[serde(default = "default_allow_default_password")]
    pub allow_default_password: bool,
}

fn default_allow_default_password() -> bool {
    false
}

fn default_dns_port() -> u16 {
    5353
} // Use 5353 in dev (53 requires root)
fn default_bind() -> String {
    "0.0.0.0".to_string()
}
fn default_api_port() -> u16 {
    8080
}
fn default_db_path() -> String {
    "./rust-dns.db".to_string()
}
fn default_jwt_expiry() -> u64 {
    24
}
fn default_cors_allowed_origins() -> Vec<String> {
    vec![
        "http://localhost:5173".to_string(),
        "http://localhost:5174".to_string(),
        "http://localhost:8080".to_string(),
    ]
}
fn default_query_log_retention_days() -> u32 {
    7
}
fn default_prefer_ipv4() -> bool {
    true
}
fn default_rewrite_ttl() -> u32 {
    300
}
fn default_static_dir() -> String {
    "frontend/dist".to_string()
}
fn default_log_level() -> String {
    "info".to_string()
}
fn default_log_rotation() -> String {
    "daily".to_string()
}
fn default_log_console() -> bool {
    true
}

const DEFAULT_JWT_SECRET: &str = "change-me-in-production";

pub fn validate(cfg: &Config) -> Result<()> {
    // Security: Reject default JWT secret
    if cfg.auth.jwt_secret == DEFAULT_JWT_SECRET {
        anyhow::bail!(
            "SECURITY ERROR: JWT secret must be changed from default value '{}'. \
            Set RUST_DNS__AUTH__JWT_SECRET environment variable with a strong random value.",
            DEFAULT_JWT_SECRET
        );
    }

    // Security: JWT secret must be at least 32 characters
    if cfg.auth.jwt_secret.len() < 32 {
        anyhow::bail!(
            "CONFIG ERROR: JWT secret must be at least 32 characters (current: {})",
            cfg.auth.jwt_secret.len()
        );
    }

    // Validate database path directory exists or can be created
    if let Some(parent) = std::path::Path::new(&cfg.database.path).parent() {
        if !parent.exists() {
            anyhow::bail!(
                "CONFIG ERROR: Database directory does not exist: {}",
                parent.display()
            );
        }
    }

    tracing::info!("Configuration validation passed");
    Ok(())
}

/// Load configuration with the following priority (highest → lowest):
///
/// 1. Environment variables (`RUST_DNS__<SECTION>__<KEY>`)
/// 2. Config file specified via `config_path` argument or `RUST_DNS_CONFIG` env var
/// 3. Auto-discovered config file from default locations (`./config.toml`, `/etc/rust-dns/config.toml`)
/// 4. Built-in defaults
pub fn load(config_path: Option<&str>) -> Result<Config> {
    // Resolve config file path: CLI arg > RUST_DNS_CONFIG env > default search paths
    let resolved_file = config_path
        .map(|p| p.to_string())
        .or_else(|| std::env::var("RUST_DNS_CONFIG").ok())
        .or_else(|| {
            DEFAULT_CONFIG_PATHS
                .iter()
                .find(|p| std::path::Path::new(p).exists())
                .map(|p| p.to_string())
        });

    let mut builder = config::Config::builder()
        // Lowest priority: built-in defaults
        .set_default("dns.bind", "0.0.0.0")?
        .set_default("dns.port", 5353)?
        .set_default("dns.upstreams", vec!["1.1.1.1:53", "8.8.8.8:53"])?
        .set_default("dns.prefer_ipv4", true)?
        .set_default("dns.doh_enabled", false)?
        .set_default("dns.dot_enabled", false)?
        .set_default("dns.rewrite_ttl", 300)?
        .set_default("api.bind", "0.0.0.0")?
        .set_default("api.port", 8080)?
        .set_default("api.static_dir", "frontend/dist")?
        .set_default("database.path", "./rust-dns.db")?
        .set_default("database.query_log_retention_days", 7)?
        .set_default("auth.jwt_secret", DEFAULT_JWT_SECRET)?
        .set_default("auth.jwt_expiry_hours", 24)?
        .set_default("logging.level", "info")?
        .set_default("logging.rotation", "daily")?
        .set_default("logging.console", true)?;

    // Mid priority: config file (if found or explicitly specified)
    match (&resolved_file, config_path) {
        (Some(path), Some(_)) => {
            // Explicitly specified — required, error if missing
            eprintln!("Loading config: {}", path);
            builder = builder.add_source(
                config::File::with_name(path)
                    .required(true)
                    .format(config::FileFormat::Toml),
            );
        }
        (Some(path), None) => {
            // Auto-discovered — optional
            eprintln!("Loading config: {}", path);
            builder = builder.add_source(
                config::File::with_name(path)
                    .required(false)
                    .format(config::FileFormat::Toml),
            );
        }
        (None, _) => {
            eprintln!("No config file found; using defaults + environment variables");
        }
    }

    // Highest priority: environment variables (always override file)
    builder = builder.add_source(config::Environment::with_prefix("RUST_DNS").separator("__"));

    let cfg: Config = builder.build()?.try_deserialize()?;

    validate(&cfg)?;

    Ok(cfg)
}
