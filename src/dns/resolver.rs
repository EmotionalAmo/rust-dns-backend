use crate::config::Config;
use anyhow::Result;
use hickory_proto::op::{Message, MessageType, OpCode, ResponseCode};
use hickory_proto::rr::RecordType;
use hickory_resolver::config::{
    NameServerConfig, NameServerConfigGroup, Protocol, ResolverConfig, ResolverOpts,
};
use hickory_resolver::error::ResolveErrorKind;
use hickory_resolver::TokioAsyncResolver;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};

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

    /// Create a resolver using custom upstreams.
    ///
    /// Accepts:
    /// - Plain IP addresses: `"192.168.1.1"`, `"8.8.8.8:53"`
    /// - DoH URLs: `"https://dns.cloudflare.com/dns-query"`, `"https://1.1.1.1/dns-query"`
    ///
    /// DoH URLs require hostname-to-IP resolution at startup (blocking via
    /// `std::net::ToSocketAddrs`). If hostname resolution fails, the entry is
    /// skipped with a warning.
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

            if upstream.starts_with("https://") {
                // --- DoH (DNS-over-HTTPS) upstream ---
                match parse_doh_url(upstream) {
                    Ok((host, port, _path)) => {
                        // Resolve hostname to IP(s) synchronously (acceptable at init time)
                        let lookup_target = format!("{}:{}", host, port);
                        match lookup_target.as_str().to_socket_addrs() {
                            Ok(addrs) => {
                                let ips: Vec<IpAddr> = addrs
                                    .filter(|a| if prefer_ipv4 { a.is_ipv4() } else { true })
                                    .map(|a| a.ip())
                                    .collect();

                                if ips.is_empty() {
                                    tracing::warn!(
                                        "DoH upstream {}: no usable IPs after resolution (prefer_ipv4={}), skipping",
                                        upstream, prefer_ipv4
                                    );
                                    continue;
                                }

                                let ns_group = NameServerConfigGroup::from_ips_https(
                                    &ips,
                                    port,
                                    host.clone(),
                                    false,
                                );
                                for ns in ns_group.into_inner() {
                                    config.add_name_server(ns);
                                    added += 1;
                                }
                                tracing::info!(
                                    "Added DoH upstream {} -> {} IP(s) at port {}",
                                    upstream,
                                    ips.len(),
                                    port
                                );
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "DoH upstream {}: failed to resolve host '{}': {}, skipping",
                                    upstream,
                                    host,
                                    e
                                );
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Invalid DoH upstream URL '{}': {}, skipping", upstream, e);
                    }
                }
            } else if upstream.starts_with("tls://") {
                // --- DoT (DNS-over-TLS) upstream ---
                match parse_dot_url(upstream) {
                    Ok((host, port)) => {
                        // Resolve hostname to IP(s) synchronously (acceptable at init time)
                        let lookup_target = format!("{}:{}", host, port);
                        match lookup_target.as_str().to_socket_addrs() {
                            Ok(addrs) => {
                                let ips: Vec<IpAddr> = addrs
                                    .filter(|a| if prefer_ipv4 { a.is_ipv4() } else { true })
                                    .map(|a| a.ip())
                                    .collect();

                                if ips.is_empty() {
                                    tracing::warn!(
                                        "DoT upstream {}: no usable IPs after resolution (prefer_ipv4={}), skipping",
                                        upstream, prefer_ipv4
                                    );
                                    continue;
                                }

                                let ns_group = NameServerConfigGroup::from_ips_tls(
                                    &ips,
                                    port,
                                    host.clone(),
                                    false,
                                );
                                for ns in ns_group.into_inner() {
                                    config.add_name_server(ns);
                                    added += 1;
                                }
                                tracing::info!(
                                    "Added DoT upstream {} -> {} IP(s) at port {}",
                                    upstream,
                                    ips.len(),
                                    port
                                );
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "DoT upstream {}: failed to resolve host '{}': {}, skipping",
                                    upstream,
                                    host,
                                    e
                                );
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Invalid DoT upstream URL '{}': {}, skipping", upstream, e);
                    }
                }
            } else {
                // --- Plain UDP upstream (ip or ip:port) ---
                let addr: Option<SocketAddr> = if let Ok(a) = upstream.parse::<SocketAddr>() {
                    if prefer_ipv4 && a.is_ipv6() {
                        tracing::debug!("Skipping IPv6 upstream due to prefer_ipv4: {}", a);
                        continue;
                    }
                    Some(a)
                } else if let Ok(ip) = upstream.parse::<IpAddr>() {
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

/// Parse a DoT URL into (host, port) components.
///
/// Supports formats:
/// - `tls://hostname` (default port 853)
/// - `tls://hostname:853`
/// - `tls://1.1.1.1`
/// - `tls://[::1]:853` (IPv6)
///
/// Returns an error string if the URL is not a valid `tls://` URL.
fn parse_dot_url(url: &str) -> std::result::Result<(String, u16), String> {
    // Strip the "tls://" prefix
    let rest = url
        .strip_prefix("tls://")
        .ok_or_else(|| format!("URL must start with tls://: {}", url))?;

    if rest.is_empty() {
        return Err(format!("Empty host in DoT URL: {}", url));
    }

    // Strip any trailing path (DoT doesn't use paths, but be defensive)
    let authority = match rest.find('/') {
        Some(idx) => &rest[..idx],
        None => rest,
    };

    // Split host from port — handle IPv6 literals like [::1] or [::1]:853
    let (host, port) = if authority.starts_with('[') {
        // IPv6 literal
        let end_bracket = authority
            .rfind(']')
            .ok_or_else(|| format!("Malformed IPv6 address in URL: {}", url))?;
        let host_part = authority[1..end_bracket].to_string();
        let port_part = &authority[end_bracket + 1..];
        let port = if port_part.is_empty() {
            853u16
        } else {
            port_part
                .strip_prefix(':')
                .and_then(|p| p.parse::<u16>().ok())
                .ok_or_else(|| format!("Invalid port in URL: {}", url))?
        };
        (host_part, port)
    } else {
        // Regular hostname or IPv4
        match authority.rfind(':') {
            Some(idx) => {
                let host_part = authority[..idx].to_string();
                let port_str = &authority[idx + 1..];
                let port = port_str
                    .parse::<u16>()
                    .map_err(|_| format!("Invalid port '{}' in URL: {}", port_str, url))?;
                (host_part, port)
            }
            None => (authority.to_string(), 853u16),
        }
    };

    if host.is_empty() {
        return Err(format!("Empty host in DoT URL: {}", url));
    }

    Ok((host, port))
}

/// Parse a DoH URL into (host, port, path) components.
///
/// Supports formats:
/// - `https://hostname/path`
/// - `https://hostname:port/path`
/// - `https://hostname` (no path)
///
/// Returns an error string if the URL is not a valid `https://` URL.
fn parse_doh_url(url: &str) -> std::result::Result<(String, u16, String), String> {
    // Strip the "https://" prefix
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| format!("URL must start with https://: {}", url))?;

    if rest.is_empty() {
        return Err(format!("Empty host in DoH URL: {}", url));
    }

    // Split authority from path at the first '/'
    let (authority, path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], rest[idx..].to_string()),
        None => (rest, "/dns-query".to_string()),
    };

    // Split host from port at the last ':' — but be careful of IPv6 literals like [::1]:443
    let (host, port) = if authority.starts_with('[') {
        // IPv6 literal: [::1] or [::1]:443
        let end_bracket = authority
            .rfind(']')
            .ok_or_else(|| format!("Malformed IPv6 address in URL: {}", url))?;
        let host_part = authority[1..end_bracket].to_string();
        let port_part = &authority[end_bracket + 1..];
        let port = if port_part.is_empty() {
            443u16
        } else {
            port_part
                .strip_prefix(':')
                .and_then(|p| p.parse::<u16>().ok())
                .ok_or_else(|| format!("Invalid port in URL: {}", url))?
        };
        (host_part, port)
    } else {
        // Regular hostname or IPv4
        match authority.rfind(':') {
            Some(idx) => {
                let host_part = authority[..idx].to_string();
                let port_str = &authority[idx + 1..];
                let port = port_str
                    .parse::<u16>()
                    .map_err(|_| format!("Invalid port '{}' in URL: {}", port_str, url))?;
                (host_part, port)
            }
            None => (authority.to_string(), 443u16),
        }
    };

    if host.is_empty() {
        return Err(format!("Empty host in DoH URL: {}", url));
    }

    Ok((host, port, path))
}
