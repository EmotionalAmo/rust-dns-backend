use crate::db::models::upstream::Upstream;
use crate::dns::resolver::DnsResolver;
use anyhow::Result;
use hickory_proto::op::Message;
use hickory_proto::rr::RecordType;
use rand::seq::SliceRandom;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpstreamStrategy {
    Priority,
    LoadBalance,
    Fastest,
}

impl UpstreamStrategy {
    pub fn from_string(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "load_balance" | "loadbalance" => Self::LoadBalance,
            "fastest" => Self::Fastest,
            _ => Self::Priority, // Default
        }
    }
}

pub struct UpstreamNode {
    pub model: Upstream,
    pub resolver: Arc<DnsResolver>,
    pub last_latency_ms: AtomicI64,
}

pub struct UpstreamPool {
    nodes: Vec<Arc<UpstreamNode>>,
    strategy: UpstreamStrategy,
}

impl UpstreamPool {
    /// Create a new UpstreamPool based on a list of DB models and a chosen strategy.
    pub fn new(upstreams: Vec<Upstream>, strategy_str: &str, prefer_ipv4: bool) -> Result<Self> {
        let strategy = UpstreamStrategy::from_string(strategy_str);
        let mut nodes = Vec::new();

        for model in upstreams {
            // Only add active and healthy (or unknown) upstreams
            if !model.is_active
                || model.health_status == "degraded"
                || model.health_status == "dead"
            {
                continue;
            }

            // Parse addresses array
            let addrs = match model.addresses_vec() {
                Ok(a) if !a.is_empty() => a,
                _ => continue,
            };

            // Instantiate resolver for this specific upstream
            match DnsResolver::with_upstreams(&addrs, prefer_ipv4) {
                Ok(resolver) => {
                    nodes.push(Arc::new(UpstreamNode {
                        model,
                        resolver: Arc::new(resolver),
                        last_latency_ms: AtomicI64::new(0), // Initial state
                    }));
                }
                Err(e) => tracing::warn!(
                    "Failed to create resolver for upstream {}: {}",
                    model.name,
                    e
                ),
            }
        }

        // Sort by priority initially
        nodes.sort_by_key(|n| n.model.priority);

        // Fallback if no active/healthy upstreams are found
        if nodes.is_empty() {
            tracing::warn!(
                "No active/healthy custom upstreams found, pool will fall back to Cloudflare"
            );
            let fallback_resolver = DnsResolver::with_upstreams(
                &["1.1.1.1:53".to_string(), "8.8.8.8:53".to_string()],
                prefer_ipv4,
            )?;
            let fallback_model = Upstream {
                id: "fallback".to_string(),
                name: "Cloudflare/Google Fallback".to_string(),
                addresses: "[\"1.1.1.1:53\", \"8.8.8.8:53\"]".to_string(),
                priority: 999,
                is_active: true,
                health_check_enabled: false,
                failover_enabled: false,
                health_check_interval: 0,
                health_check_timeout: 0,
                failover_threshold: 0,
                health_status: "healthy".to_string(),
                last_health_check_at: None,
                last_failover_at: None,
                created_at: chrono::Utc::now().to_rfc3339(),
                updated_at: chrono::Utc::now().to_rfc3339(),
            };
            nodes.push(Arc::new(UpstreamNode {
                model: fallback_model,
                resolver: Arc::new(fallback_resolver),
                last_latency_ms: AtomicI64::new(0),
            }));
        }

        Ok(Self { nodes, strategy })
    }

    /// Update latency stats dynamically from background pings
    pub fn update_latency(&mut self, upstream_id: &str, latency_ms: i64) {
        for node in &mut self.nodes {
            if node.model.id == upstream_id {
                node.last_latency_ms.store(latency_ms, Ordering::Relaxed);
            }
        }
    }

    /// Select an upstream based on the current strategy and resolve the query.
    /// On failure, falls back to remaining nodes in priority order.
    /// Returns: (response_bytes, min_ttl, upstream_name)
    pub async fn resolve(
        &self,
        domain: &str,
        qtype: RecordType,
        request: &Message,
    ) -> Result<(Vec<u8>, Option<u32>, Option<String>)> {
        // Build an ordered list of nodes to try
        let nodes_to_try: Vec<Arc<UpstreamNode>> = match self.strategy {
            UpstreamStrategy::Priority => {
                // Try all nodes in priority order (sorted at construction)
                self.nodes.clone()
            }
            UpstreamStrategy::LoadBalance => {
                // Start from a random node, then try others
                let mut rng = rand::thread_rng();
                let mut nodes = self.nodes.clone();
                nodes.shuffle(&mut rng);
                nodes
            }
            UpstreamStrategy::Fastest => {
                // Sort by latency ascending
                let mut nodes = self.nodes.clone();
                nodes.sort_by_key(|n| n.last_latency_ms.load(Ordering::Relaxed));
                nodes
            }
        };

        let mut last_err = anyhow::anyhow!("No upstream nodes available");
        for node in &nodes_to_try {
            tracing::debug!("UpstreamPool: trying {} for {}", node.model.name, domain);
            match node.resolver.resolve(domain, qtype, request).await {
                Ok((res, min_ttl, _)) => {
                    return Ok((res, min_ttl, Some(node.model.name.clone())));
                }
                Err(e) => {
                    tracing::warn!(
                        "Upstream {} failed for {}: {}; trying next",
                        node.model.name,
                        domain,
                        e
                    );
                    last_err = e;
                }
            }
        }
        Err(last_err)
    }

    // Removed unused select_upstream method
}
