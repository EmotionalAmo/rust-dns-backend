use crate::config::Config;
use anyhow::Result;
use hickory_proto::op::{Message, MessageType, OpCode, ResponseCode};
use hickory_proto::rr::RecordType;
use hickory_resolver::config::{NameServerConfig, Protocol, ResolverConfig, ResolverOpts};
use hickory_resolver::error::ResolveErrorKind;
use hickory_resolver::TokioAsyncResolver;
use std::net::SocketAddr;

pub struct DnsResolver {
    inner: TokioAsyncResolver,
}

impl DnsResolver {
    /// Default resolver using Cloudflare (plain UDP fallback for dev compatibility).
    ///
    /// Note: DNSSEC validation is DISABLED for performance and compatibility.
    /// Cloudflare's DoH endpoints may not return DNSSEC signatures for all queries,
    /// and enabling validation would cause SERVFAIL responses for many domains.
    pub async fn new(cfg: &Config) -> Result<Self> {
        let ip_strategy = if cfg.dns.prefer_ipv4 {
            hickory_resolver::config::LookupIpStrategy::Ipv4Only
        } else {
            hickory_resolver::config::LookupIpStrategy::Ipv4AndIpv6
        };

        let mut opts = ResolverOpts::default();
        opts.cache_size = 0; // We handle caching ourselves
        opts.use_hosts_file = false;
        // DNSSEC validation disabled for compatibility with Cloudflare DoH
        opts.validate = false;
        // Prefer IPv4 if configured (avoids IPv6 connection issues)
        opts.ip_strategy = ip_strategy;
        // Increase concurrent upstream connections for better burst handling
        opts.num_concurrent_reqs = 8;
        // Longer timeout tolerance (upstream may be slow under load)
        opts.timeout = std::time::Duration::from_secs(5);

        let resolver = TokioAsyncResolver::tokio(ResolverConfig::cloudflare(), opts);

        tracing::info!(
            "DNS resolver initialized without DNSSEC validation, ip_strategy: {:?}, upstreams: {:?}",
            ip_strategy,
            cfg.dns.upstreams
        );
        Ok(Self { inner: resolver })
    }

    /// Create a resolver using custom upstream IPs (plain UDP port 53).
    /// Accepts IP addresses like ["192.168.1.1", "8.8.8.8"].
    pub fn with_upstreams(upstreams: &[String], prefer_ipv4: bool) -> Result<Self> {
        let ip_strategy = if prefer_ipv4 {
            hickory_resolver::config::LookupIpStrategy::Ipv4Only
        } else {
            hickory_resolver::config::LookupIpStrategy::Ipv4AndIpv6
        };

        let mut opts = ResolverOpts::default();
        opts.cache_size = 0;
        opts.use_hosts_file = false;
        opts.ip_strategy = ip_strategy;
        opts.num_concurrent_reqs = 8;
        opts.timeout = std::time::Duration::from_secs(5);

        let mut config = ResolverConfig::new();
        let mut added = 0;

        for upstream in upstreams {
            let upstream = upstream.trim();
            // Accept "ip" or "ip:port" format
            let addr: Option<SocketAddr> = if let Ok(a) = upstream.parse::<SocketAddr>() {
                // Filter out IPv6 addresses if prefer_ipv4 is true
                if prefer_ipv4 && a.is_ipv6() {
                    tracing::debug!("Skipping IPv6 upstream due to prefer_ipv4: {}", a);
                    continue;
                }
                Some(a)
            } else if let Ok(ip) = upstream.parse::<std::net::IpAddr>() {
                if prefer_ipv4 && ip.is_ipv6() {
                    tracing::debug!("Skipping IPv6 upstream due to prefer_ipv4: {}", ip);
                    continue;
                }
                Some(SocketAddr::new(ip, 53))
            } else {
                tracing::warn!("Invalid upstream address, skipping: {}", upstream);
                None
            };

            if let Some(addr) = addr {
                config.add_name_server(NameServerConfig::new(addr, Protocol::Udp));
                added += 1;
            }
        }

        if added == 0 {
            // Fall back to Cloudflare if no valid upstreams provided
            tracing::warn!("No valid custom upstreams, falling back to Cloudflare");
            return Ok(Self {
                inner: TokioAsyncResolver::tokio(ResolverConfig::cloudflare(), opts),
            });
        }

        tracing::info!(
            "Created resolver with {} upstream(s), ip_strategy: {:?}",
            added,
            ip_strategy
        );

        Ok(Self {
            inner: TokioAsyncResolver::tokio(config, opts),
        })
    }

    /// Resolve a DNS query.  Returns the serialised DNS wire format response
    /// together with the minimum TTL extracted from the answer records so the
    /// caller can store it in the cache with a matching expiry.
    pub async fn resolve(
        &self,
        domain: &str,
        qtype: RecordType,
        request: &Message,
    ) -> Result<(Vec<u8>, Option<u32>)> {
        let request_id = request.id();
        tracing::debug!("resolver: received request with id={}", request_id);

        let mut response = Message::new();
        response.set_id(request_id);
        tracing::debug!(
            "resolver: set response id to {} (from request id {})",
            response.id(),
            request_id
        );
        response.set_message_type(MessageType::Response);
        response.set_op_code(OpCode::Query);
        response.set_recursion_desired(request.recursion_desired());
        response.set_recursion_available(true);
        for query in request.queries() {
            response.add_query(query.clone());
        }

        let mut min_ttl: Option<u32> = None;

        match self.inner.lookup(domain, qtype).await {
            Ok(lookup) => {
                tracing::debug!("lookup returned {} records", lookup.records().len());
                response.set_response_code(ResponseCode::NoError);
                for record in lookup.records() {
                    // Track minimum TTL across all answer records (Task 2)
                    let ttl = record.ttl();
                    min_ttl = Some(match min_ttl {
                        None => ttl,
                        Some(current) => current.min(ttl),
                    });
                    response.add_answer(record.clone());
                }
                tracing::debug!(
                    "After adding answers: response id={}, answer count={}",
                    response.id(),
                    response.answer_count()
                );
                tracing::debug!(
                    "Resolved {} {:?}: {} records, min_ttl={:?}",
                    domain,
                    qtype,
                    response.answer_count(),
                    min_ttl,
                );
            }
            Err(e) => match e.kind() {
                ResolveErrorKind::NoRecordsFound { response_code, .. } => {
                    response.set_response_code(*response_code);
                    tracing::debug!("No records for {} {:?}: {:?}", domain, qtype, response_code);
                }
                _ => {
                    tracing::warn!("Upstream resolver error for {} {:?}: {}", domain, qtype, e);
                    response.set_response_code(ResponseCode::ServFail);
                }
            },
        }

        let final_id = response.id();
        tracing::debug!(
            "resolver: returning response id={}, request_id={}, final_id={}",
            final_id,
            request_id,
            final_id
        );

        Ok((response.to_vec()?, min_ttl))
    }
}
