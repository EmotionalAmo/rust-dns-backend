use super::{
    acl::Acl,
    cache::{DnsCache, DNS_CACHE_MAX_CAPACITY},
    filter::FilterEngine,
    resolver::DnsResolver,
    rules::RuleSet,
    upstream_pool::UpstreamPool,
};
use crate::config::Config;
use crate::db::query_log_writer::QueryLogEntry;
use crate::db::DbPool;
use crate::metrics::DnsMetrics;
use anyhow::Result;
use bytes::Bytes;
use chrono::Utc;
use hickory_proto::op::{Message, MessageType, OpCode, ResponseCode};
use hickory_proto::rr::{
    rdata::{A, AAAA},
    RData, Record, RecordType,
};
use moka::future::Cache as MokaCache;
use std::borrow::Cow;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc, RwLock};

/// TTL for the client-config cache.  Client configs change rarely; 60 s is safe. (M-4 fix)
const CLIENT_CACHE_TTL: Duration = Duration::from_secs(60);

/// Per-client resolved configuration, cached in moka for CLIENT_CACHE_TTL.
#[derive(Clone)]
struct ClientConfig {
    /// Whether DNS filtering is enabled for this client.
    filter_enabled: bool,
    /// Custom upstream resolvers, if specified by the client or its highest-priority group.
    upstream_urls: Option<Vec<String>>,
    /// Group-specific rule set built from client_group_rules → custom_rules.
    /// When Some, replaces the global FilterEngine check for this client.
    /// When None, falls back to the global FilterEngine.
    group_ruleset: Option<Arc<RuleSet>>,
    /// Group-specific DNS rewrites built from client_group_rules → dns_rewrites.
    /// Checked before global filter rewrites.
    group_rewrites: Option<Arc<HashMap<String, String>>>,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            filter_enabled: true,
            upstream_urls: None,
            group_ruleset: None,
            group_rewrites: None,
        }
    }
}

pub struct DnsHandler {
    filter: Arc<FilterEngine>,
    upstream_pool: Arc<RwLock<UpstreamPool>>,
    prefer_ipv4: bool,
    /// TTL（秒）用于 DNS 重写响应（可通过 dns.rewrite_ttl 配置）
    rewrite_ttl: u32,
    /// DNS ACL — 控制哪些客户端 IP 允许查询此服务器
    acl: Arc<RwLock<Acl>>,
    /// Per-client resolvers keyed by sorted upstream list (e.g. "1.1.1.1,8.8.8.8")
    /// Bounded moka cache: max 64 entries, TTL 1 hour — prevents unbounded growth
    client_resolvers: MokaCache<String, Arc<DnsResolver>>,
    cache: Arc<DnsCache>,
    /// TTL cache for client config: IP → ClientConfig (M-4 fix)
    client_config_cache: MokaCache<String, ClientConfig>,
    db: DbPool,
    metrics: Arc<DnsMetrics>,
    query_log_tx: broadcast::Sender<serde_json::Value>,
    /// Non-blocking sender to the batch query log writer (bounded channel, OOM 防护)
    query_log_entry_tx: mpsc::Sender<QueryLogEntry>,
    app_catalog: Arc<crate::db::app_catalog_cache::AppCatalogCache>,
}

impl DnsHandler {
    pub async fn new(
        cfg: Config,
        db: DbPool,
        filter: Arc<FilterEngine>,
        metrics: Arc<DnsMetrics>,
        query_log_tx: broadcast::Sender<serde_json::Value>,
        app_catalog: Arc<crate::db::app_catalog_cache::AppCatalogCache>,
    ) -> Result<Self> {
        // Intialize UpstreamPool from database
        let upstreams = crate::db::models::upstream::UpstreamRepository::list(&db).await?;

        let strategy = sqlx::query_scalar::<_, String>(
            "SELECT value FROM settings WHERE key = 'upstream_strategy'",
        )
        .fetch_optional(&db)
        .await?
        .unwrap_or_else(|| "priority".to_string());

        let upstream_pool = Arc::new(RwLock::new(UpstreamPool::new(
            upstreams,
            &strategy,
            cfg.dns.prefer_ipv4,
        )?));

        let cache = Arc::new(DnsCache::new());
        let client_config_cache = MokaCache::builder()
            .max_capacity(4096)
            .time_to_live(CLIENT_CACHE_TTL)
            .build();
        // Spawn batch writer; the sender is stored so log_query() is fully non-blocking
        let query_log_entry_tx =
            crate::db::query_log_writer::spawn(db.clone(), query_log_tx.clone());
        let prefer_ipv4 = cfg.dns.prefer_ipv4;
        let rewrite_ttl = cfg.dns.rewrite_ttl;

        // Load ACL from database settings
        let acl = {
            let allowed = sqlx::query_scalar::<_, String>(
                "SELECT value FROM settings WHERE key = 'acl_allowed_networks'",
            )
            .fetch_optional(&db)
            .await?
            .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
            .unwrap_or_default();

            let denied = sqlx::query_scalar::<_, String>(
                "SELECT value FROM settings WHERE key = 'acl_denied_networks'",
            )
            .fetch_optional(&db)
            .await?
            .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
            .unwrap_or_default();

            Arc::new(RwLock::new(Acl::from_cidrs(&allowed, &denied)))
        };

        Ok(Self {
            filter,
            upstream_pool,
            prefer_ipv4,
            rewrite_ttl,
            acl,
            client_resolvers: MokaCache::builder()
                .max_capacity(64)
                .time_to_live(std::time::Duration::from_secs(3600))
                .build(),
            cache,
            client_config_cache,
            db,
            metrics,
            query_log_tx,
            query_log_entry_tx,
            app_catalog,
        })
    }

    /// Handle a DNS query (wire format bytes).  Used by both UDP and TCP transports.
    pub async fn handle(&self, data: Vec<u8>, client_ip: String) -> Result<Vec<u8>> {
        self.handle_internal(data.as_ref(), &client_ip).await
    }

    /// Handle a DNS query from Bytes (zero-copy for UDP worker pool).
    /// Used by UDP path to avoid unnecessary allocation.
    pub async fn handle_bytes(&self, data: Bytes, client_ip: &str) -> Result<Vec<u8>> {
        self.handle_internal(data.as_ref(), client_ip).await
    }

    /// Internal handler that works with &[u8] to avoid allocations.
    async fn handle_internal(&self, data: &[u8], client_ip: &str) -> Result<Vec<u8>> {
        let request = Message::from_vec(data)?;

        tracing::debug!(
            "REQ: id={} type={:?} opcode={:?} queries={}",
            request.id(),
            request.message_type(),
            request.op_code(),
            request.queries().len()
        );

        // ACL check — refuse queries from disallowed clients
        if let Ok(ip) = client_ip.parse::<IpAddr>() {
            if !self.acl.read().await.is_allowed(ip) {
                tracing::warn!("ACL: rejected DNS query from {}", client_ip);
                return self.refused(&request);
            }
        }

        if request.message_type() != MessageType::Query || request.op_code() != OpCode::Query {
            return self.servfail(&request);
        }

        let query = match request.queries().first() {
            Some(q) => q,
            None => return self.servfail(&request),
        };

        let domain = query.name().to_string();
        let qtype = query.query_type();
        let qtype_str = format!("{:?}", qtype);
        let start = Instant::now();

        tracing::debug!("Query: {} {:?} from {}", domain, qtype, client_ip);

        // Normalize domain (remove trailing dot)
        let domain_normalized = domain.trim_end_matches('.');

        // Look up client-specific config (filter override + custom upstreams + group rules/rewrites)
        let config = self.get_client_config(client_ip).await;

        // Check DNS rewrite (group-specific rewrites take precedence over global)
        // check_rewrite() 是同步方法（arc-swap 无锁读），无需 .await
        let rewrite_answer = if let Some(ref rewrites) = config.group_rewrites {
            if let Some(ans) = rewrites.get(domain_normalized).cloned() {
                Some(ans)
            } else {
                self.filter.check_rewrite(domain_normalized)
            }
        } else {
            self.filter.check_rewrite(domain_normalized)
        };

        if let Some(answer) = rewrite_answer {
            tracing::debug!("Rewrite: {} -> {}", domain, answer);
            let elapsed = start.elapsed().as_nanos() as i64;

            if matches!(qtype, RecordType::A | RecordType::AAAA) {
                if let Ok(response) = self.rewrite_response(&request, &answer, qtype, &domain) {
                    self.metrics.inc_allowed();
                    self.log_query(
                        Cow::Borrowed(client_ip),
                        Cow::Borrowed(&domain),
                        Cow::Borrowed(&qtype_str),
                        Cow::Borrowed("allowed"),
                        Some(Cow::Borrowed("rewrite")),
                        Some(answer.to_string()),
                        elapsed,
                        None,
                        Some("rewrite".to_string()),
                        self.app_catalog.match_domain(domain_normalized),
                    );
                    return Ok(response);
                }
            }
        }

        if config.filter_enabled {
            let blocked = if let Some(ref ruleset) = config.group_ruleset {
                // Client belongs to a group; check specific rules first
                tracing::debug!("Checking group-specific ruleset for {}", client_ip);
                match ruleset.match_domain(domain_normalized) {
                    crate::dns::rules::MatchResult::Blocked => true,
                    crate::dns::rules::MatchResult::Allowed => false,
                    crate::dns::rules::MatchResult::None => {
                        // Fallback to global FilterEngine if group rules didn't explicitly allow/block
                        // is_blocked() 是同步方法（arc-swap 无锁读），无需 .await
                        self.filter.is_blocked(domain_normalized)
                    }
                }
            } else {
                // No group rules — use global FilterEngine
                // is_blocked() 是同步方法（arc-swap 无锁读），无需 .await
                self.filter.is_blocked(domain_normalized)
            };

            if blocked {
                tracing::debug!("Blocked: {}", domain);
                let elapsed = start.elapsed().as_nanos() as i64;
                self.metrics.inc_blocked();
                self.log_query(
                    Cow::Borrowed(client_ip),
                    Cow::Borrowed(&domain),
                    Cow::Borrowed(&qtype_str),
                    Cow::Borrowed("blocked"),
                    Some(Cow::Borrowed("filter_rule")),
                    None,
                    elapsed,
                    None,
                    None,
                    self.app_catalog.match_domain(domain_normalized),
                );
                return self.nxdomain(&request);
            }
        }

        // Check cache
        if let Some(cached) = self.cache.get(&domain, qtype).await {
            let elapsed = start.elapsed().as_nanos() as i64;

            // OPTIMIZATION: Update cached response ID by modifying bytes directly
            // DNS message ID is the first 2 bytes (big-endian). Avoid parsing+serializing.
            let mut updated_cached = cached.to_vec();
            let req_id_bytes = request.id().to_be_bytes();
            // DNS header is always ≥ 12 bytes; guard defensively against any edge case.
            debug_assert!(
                updated_cached.len() >= 12,
                "cached DNS response too short: {} bytes",
                updated_cached.len()
            );
            if updated_cached.len() >= 2 {
                updated_cached[0] = req_id_bytes[0];
                updated_cached[1] = req_id_bytes[1];
            }

            self.metrics.inc_cached();
            self.log_query(
                Cow::Borrowed(client_ip),
                Cow::Borrowed(&domain),
                Cow::Borrowed(&qtype_str),
                Cow::Borrowed("cached"),
                None,
                None,
                elapsed,
                None,
                None,
                self.app_catalog.match_domain(domain_normalized),
            );
            return Ok(updated_cached);
        }

        // Resolve: use client-specific upstream if configured, else global upstream_pool
        let upstream_start = Instant::now();
        let (response, min_ttl, upstream_name) = if let Some(ref upstreams) = config.upstream_urls {
            let resolver = self.get_or_create_client_resolver(upstreams).await?;
            // For client-specific upstreams, use the first address as the upstream name
            let name = upstreams
                .first()
                .cloned()
                .unwrap_or_else(|| "custom".to_string());
            let (res, min_ttl, _) = resolver.resolve(&domain, qtype, &request).await?;
            (res, min_ttl, Some(name))
        } else {
            let pool = self.upstream_pool.read().await;
            pool.resolve(&domain, qtype, &request).await?
        };
        let upstream_ns = upstream_start.elapsed().as_nanos() as i64;

        // Verify response ID matches request ID (CRITICAL for DNS protocol)
        let response_msg = Message::from_vec(&response)?;
        tracing::debug!(
            "DNS: domain={} req_id={} resp_id={} status={}",
            domain,
            request.id(),
            response_msg.id(),
            if response_msg.id() == request.id() {
                "MATCH"
            } else {
                "MISMATCH"
            }
        );
        if response_msg.id() != request.id() {
            tracing::error!(
                "CRITICAL: DNS ID mismatch! domain={} req_id={} resp_id={}",
                domain,
                request.id(),
                response_msg.id()
            );
        }

        let elapsed = start.elapsed().as_nanos() as i64;
        let answer_str = Self::extract_answer(&response_msg);
        // Cache with upstream-derived TTL (Task 2: respect upstream TTL)
        self.cache
            .set_with_ttl(&domain, qtype, response.clone(), min_ttl)
            .await;
        self.metrics.inc_allowed();
        self.log_query(
            Cow::Borrowed(client_ip),
            Cow::Borrowed(&domain),
            Cow::Borrowed(&qtype_str),
            Cow::Borrowed("allowed"),
            None,
            answer_str,
            elapsed,
            Some(upstream_ns),
            upstream_name,
            self.app_catalog.match_domain(domain_normalized),
        );

        Ok(response)
    }

    /// Look up client configuration by source IP.
    /// Returns ClientConfig with filter_enabled, upstream_urls, and optional group_ruleset.
    /// Results are cached for CLIENT_CACHE_TTL to avoid per-query DB scans (M-4 fix).
    async fn get_client_config(&self, client_ip: &str) -> ClientConfig {
        // Fast path: cache hit
        if let Some(cached) = self.client_config_cache.get(client_ip).await {
            return cached;
        }

        // Slow path: DB lookup (only on cache miss)
        let result = self.resolve_client_config(client_ip).await;
        self.client_config_cache
            .insert(client_ip.to_string(), result.clone())
            .await;
        result
    }

    async fn resolve_client_config(&self, client_ip: &str) -> ClientConfig {
        let full_rows: Vec<(String, String, i32, Option<String>)> =
            match sqlx::query_as("SELECT id, identifiers, filter_enabled, upstreams FROM clients")
                .fetch_all(&self.db)
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("Failed to load client config from DB: {}", e);
                    return ClientConfig::default();
                }
            };

        let mut matched_client_id: Option<String> = None;
        let mut filter_enabled = true;
        let mut upstream_urls: Option<Vec<String>> = None;

        for (client_id, identifiers_json, fe, upstreams_json) in full_rows {
            // Parse identifiers array (["192.168.1.10", "192.168.1.0/24", ...])
            if let Ok(identifiers) =
                serde_json::from_str::<Vec<serde_json::Value>>(&identifiers_json)
            {
                let matched = identifiers.iter().any(|id| {
                    let id_str = id.as_str().unwrap_or("");
                    // Exact IP match
                    if id_str == client_ip {
                        return true;
                    }
                    // CIDR match
                    if let Ok(network) = id_str.parse::<ipnet::IpNet>() {
                        if let Ok(ip) = client_ip.parse::<IpAddr>() {
                            return network.contains(&ip);
                        }
                    }
                    false
                });

                if matched {
                    matched_client_id = Some(client_id);
                    filter_enabled = fe == 1;
                    upstream_urls = upstreams_json
                        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
                        .filter(|v| !v.is_empty());
                    break;
                }
            }
        }

        // If client was matched, check for group-specific rules and rewrites
        let (group_ruleset, group_rewrites) = if let Some(ref cid) = matched_client_id {
            let ruleset = self.load_group_rules_for_client(cid).await;
            let rewrites = self.load_group_rewrites_for_client(cid).await;
            (ruleset, rewrites)
        } else {
            (None, None)
        };

        ClientConfig {
            filter_enabled,
            upstream_urls,
            group_ruleset,
            group_rewrites,
        }
    }

    /// Load rewrite rules bound to groups this client belongs to.
    async fn load_group_rewrites_for_client(
        &self,
        client_id: &str,
    ) -> Option<Arc<HashMap<String, String>>> {
        let rows: Vec<(String, String)> = match sqlx::query_as(
            r#"
            SELECT dr.domain, dr.answer
            FROM client_group_memberships m
            JOIN client_groups cg ON cg.id = m.group_id
            JOIN client_group_rules cgr ON cgr.group_id = m.group_id
            JOIN dns_rewrites dr ON dr.id = cgr.rule_id
            WHERE m.client_id = $1
              AND cgr.rule_type = 'rewrite'
            ORDER BY cg.priority ASC
            "#,
        )
        .bind(client_id)
        .fetch_all(&self.db)
        .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(
                    "Failed to load group rewrites for client {}: {}",
                    client_id,
                    e
                );
                return None;
            }
        };

        if rows.is_empty() {
            return None;
        }

        let mut rewrites = HashMap::new();
        for (domain, answer) in rows {
            rewrites.insert(domain.to_lowercase(), answer);
        }
        tracing::debug!(
            "Loaded {} group rewrites for client {}",
            rewrites.len(),
            client_id
        );
        Some(Arc::new(rewrites))
    }

    /// Load custom rule strings bound to groups this client belongs to.
    /// Fetches rules from all groups the client is in, ordered by group priority.
    /// Returns None if the client has no group rules (caller falls back to global FilterEngine).
    async fn load_group_rules_for_client(&self, client_id: &str) -> Option<Arc<RuleSet>> {
        let rule_rows: Vec<(String,)> = match sqlx::query_as(
            r#"
            SELECT cr.rule
            FROM client_group_memberships m
            JOIN client_groups cg ON cg.id = m.group_id
            JOIN client_group_rules cgr ON cgr.group_id = m.group_id
            JOIN custom_rules cr ON cr.id = cgr.rule_id
            WHERE m.client_id = $1
              AND cgr.rule_type = 'custom_rule'
              AND cr.is_enabled = 1
            ORDER BY cg.priority ASC
            "#,
        )
        .bind(client_id)
        .fetch_all(&self.db)
        .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Failed to load group rules for client {}: {}", client_id, e);
                return None;
            }
        };

        if rule_rows.is_empty() {
            return None;
        }

        let mut ruleset = RuleSet::new();
        for (rule,) in rule_rows {
            ruleset.add_rule(&rule);
        }
        tracing::debug!(
            "Loaded {} group rules for client {}",
            ruleset.blocked_count() + ruleset.allowed_count(),
            client_id
        );
        Some(Arc::new(ruleset))
    }

    /// Get or create a cached per-client resolver for the given upstream list.
    /// Uses a bounded moka cache (max 64, TTL 1h) to prevent unbounded growth.
    async fn get_or_create_client_resolver(
        &self,
        upstreams: &[String],
    ) -> Result<Arc<DnsResolver>> {
        let key = {
            let mut sorted = upstreams.to_vec();
            sorted.sort();
            sorted.join(",")
        };

        if let Some(r) = self.client_resolvers.get(&key).await {
            return Ok(r);
        }

        let resolver = Arc::new(DnsResolver::with_upstreams(upstreams, self.prefer_ipv4)?);
        self.client_resolvers.insert(key, resolver.clone()).await;
        tracing::info!("Created client resolver for upstreams: {:?}", upstreams);
        Ok(resolver)
    }

    /// Build a response for DNS rewrite
    fn rewrite_response(
        &self,
        request: &Message,
        answer: &str,
        qtype: RecordType,
        domain: &str,
    ) -> Result<Vec<u8>> {
        let ip: IpAddr = answer
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid IP address: {}", answer))?;

        let rdata = match (ip, qtype) {
            (IpAddr::V4(ipv4), RecordType::A) => RData::A(A(ipv4)),
            (IpAddr::V6(ipv6), RecordType::AAAA) => RData::AAAA(AAAA(ipv6)),
            _ => anyhow::bail!("IP type doesn't match query type"),
        };

        let query = request
            .queries()
            .first()
            .ok_or_else(|| anyhow::anyhow!("DNS rewrite request contains no queries"))?;

        let mut record = Record::new();
        record.set_name(query.name().clone());
        record.set_record_type(qtype);
        record.set_ttl(self.rewrite_ttl);
        record.set_data(Some(rdata));

        let mut response = Message::new();
        response.set_id(request.id());
        response.set_message_type(MessageType::Response);
        response.set_response_code(ResponseCode::NoError);
        response.set_recursion_desired(request.recursion_desired());
        response.set_recursion_available(true);
        response.add_query(query.clone());
        response.add_answer(record);

        tracing::debug!(
            "REWRITE: req_id={} resp_id={} domain={} -> {}",
            request.id(),
            response.id(),
            domain,
            answer
        );

        Ok(response.to_vec()?)
    }

    /// Non-blocking query log write + WebSocket broadcast.
    ///
    /// The DB write goes through the batch writer (Task 1): send() is O(1) and
    /// never blocks the DNS hot path.  The WebSocket broadcast is also fire-and-forget.
    #[allow(clippy::too_many_arguments)]
    fn log_query(
        &self,
        client_ip: Cow<str>,
        domain: Cow<str>,
        qtype: Cow<str>,
        status: Cow<str>,
        reason: Option<Cow<str>>,
        answer: Option<String>,
        elapsed_ns: i64,
        upstream_ns: Option<i64>,
        upstream_name: Option<String>,
        app_id: Option<i32>,
    ) {
        let now = Utc::now().to_rfc3339();

        // Enqueue for batch write — non-blocking (bounded channel，满了静默丢弃)
        // Convert Cow to String only for DB storage (required by QueryLogEntry)
        let entry = QueryLogEntry {
            time: now.clone(),
            client_ip: client_ip.to_string(),
            question: domain.to_string(),
            qtype: qtype.to_string(),
            status: status.to_string(),
            reason: reason.as_ref().map(|r| r.to_string()),
            answer: answer.clone(),
            elapsed_ns,
            upstream_ns,
            upstream_name: upstream_name.clone(),
            app_id,
        };
        if let Err(e) = self.query_log_entry_tx.try_send(entry) {
            tracing::warn!(
                "QueryLogWriter channel full or closed, dropping entry: {}",
                e
            );
        }

        // WebSocket real-time broadcast (non-blocking; receivers may not exist)
        // Use Cow directly for JSON serialization (avoid extra allocations)
        let event = serde_json::json!({
            "time": now,
            "client_ip": &client_ip,
            "question": &domain,
            "qtype": &qtype,
            "status": &status,
            "reason": &reason,
            "answer": &answer,
            "elapsed_ns": elapsed_ns,
            "upstream_ns": upstream_ns,
            "upstream": upstream_name,
        });
        let _ = self.query_log_tx.send(event);
    }

    /// Extract answer records from a DNS response as a comma-separated string.
    fn extract_answer(msg: &Message) -> Option<String> {
        let parts: Vec<String> = msg
            .answers()
            .iter()
            .filter_map(|r| r.data().map(|d| d.to_string()))
            .take(5)
            .collect();
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(", "))
        }
    }

    fn nxdomain(&self, request: &Message) -> Result<Vec<u8>> {
        let mut response = Message::new();
        response.set_id(request.id());
        response.set_message_type(MessageType::Response);
        response.set_response_code(ResponseCode::NXDomain);
        response.set_recursion_desired(request.recursion_desired());
        response.set_recursion_available(true);
        for query in request.queries() {
            response.add_query(query.clone());
        }
        Ok(response.to_vec()?)
    }

    fn servfail(&self, request: &Message) -> Result<Vec<u8>> {
        let mut response = Message::new();
        response.set_id(request.id());
        response.set_message_type(MessageType::Response);
        response.set_response_code(ResponseCode::ServFail);
        Ok(response.to_vec()?)
    }

    fn refused(&self, request: &Message) -> Result<Vec<u8>> {
        let mut response = Message::new();
        response.set_id(request.id());
        response.set_message_type(MessageType::Response);
        response.set_response_code(ResponseCode::Refused);
        Ok(response.to_vec()?)
    }

    /// Hot-reload ACL rules without restarting.  Called by the settings API.
    pub async fn reload_acl(&self, allowed: Vec<String>, denied: Vec<String>) {
        let new_acl = Acl::from_cidrs(&allowed, &denied);
        *self.acl.write().await = new_acl;
        tracing::info!(
            "ACL reloaded: {} allowed, {} denied network(s)",
            allowed.len(),
            denied.len()
        );
    }

    /// Returns DNS cache statistics as (entry_count, max_capacity).
    pub async fn cache_stats(&self) -> (u64, u64) {
        (self.cache.entry_count(), DNS_CACHE_MAX_CAPACITY)
    }

    /// Flushes all entries from the DNS cache.
    pub async fn cache_flush(&self) {
        self.cache.invalidate_all().await;
    }

    /// 失效指定客户端 IP 的配置缓存（P1-1 fix）。
    /// 当 API 层修改分组成员或规则后调用，确保 DNS 引擎立即使用最新配置。
    pub async fn invalidate_client_cache(&self, client_ip: &str) {
        self.client_config_cache.invalidate(client_ip).await;
    }

    /// 失效全部客户端配置缓存（P1-1 fix）。
    /// 用于分组批量操作（无法快速枚举所有受影响 IP 时），代价可控：
    /// cache miss 仅触发一次 DB 查询，TTL=60s 内会重新缓存。
    pub async fn invalidate_all_client_cache(&self) {
        self.client_config_cache.invalidate_all();
    }

    /// Reload global upstreams from database (called when settings change).
    pub async fn reload_upstreams(&self) -> Result<()> {
        tracing::info!("Reloading upstream pool from database...");
        let upstreams = crate::db::models::upstream::UpstreamRepository::list(&self.db).await?;
        let strategy = sqlx::query_scalar::<_, String>(
            "SELECT value FROM settings WHERE key = 'upstream_strategy'",
        )
        .fetch_optional(&self.db)
        .await?
        .unwrap_or_else(|| "priority".to_string());

        let new_pool = UpstreamPool::new(upstreams, &strategy, self.prefer_ipv4)?;

        let mut pool = self.upstream_pool.write().await;
        *pool = new_pool;
        tracing::info!("Upstream pool reloaded successfully");
        Ok(())
    }

    /// Run health checks against all upstreams that have health_check_enabled.
    /// Delegates to UpstreamPool::health_check_all which has access to internal nodes.
    /// Returns (upstream_id, latency_ms, is_healthy) per checked upstream.
    pub async fn health_check_upstreams(&self) -> Vec<(String, i64, bool)> {
        let pool = self.upstream_pool.read().await;
        pool.health_check_all().await
    }

    /// Update the latency of a specific upstream dynamically via background health checks.
    pub async fn update_upstream_latency(&self, upstream_id: &str, latency_ms: i64) {
        let mut pool = self.upstream_pool.write().await;
        pool.update_latency(upstream_id, latency_ms);
    }
}
