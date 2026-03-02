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
    pub fn from_str(s: &str) -> Self {
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
        let strategy = UpstreamStrategy::from_str(strategy_str);
        let mut nodes = Vec::new();

        for model in upstreams {
            // Only add active and healthy (or unknown) upstreams
            if !model.is_active || model.health_status == "degraded" || model.health_status == "dead" {
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
                Err(e) => tracing::warn!("Failed to create resolver for upstream {}: {}", model.name, e),
            }
        }

        // Sort by priority initially
        nodes.sort_by_key(|n| n.model.priority);

        // Fallback if no active/healthy upstreams are found
        if nodes.is_empty() {
            tracing::warn!("No active/healthy custom upstreams found, pool will fall back to Cloudflare");
            let fallback_resolver = DnsResolver::with_upstreams(&["1.1.1.1:53".to_string(), "8.8.8.8:53".to_string()], prefer_ipv4)?;
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

    /// Select an upstream based on the current strategy and resolve the query
    pub async fn resolve(
        &self,
        domain: &str,
        qtype: RecordType,
        request: &Message,
    ) -> Result<(Vec<u8>, Option<u32>)> {
        let selected_node = self.select_upstream();
        
        tracing::debug!(
            "UpstreamPool strategy {:?} selected {} for domain {}",
            self.strategy,
            selected_node.model.name,
            domain
        );

        match selected_node.resolver.resolve(domain, qtype, request).await {
            Ok(res) => Ok(res),
            Err(e) => {
                tracing::warn!("Upstream {} failed to resolve {}: {}", selected_node.model.name, domain, e);
                // On failure, if using Priority strategy, we could try the next one.
                // For simplicity in this first iteration, we just return the error 
                // and let the client retry, while the background health task marks it degraded.
                Err(e)
            }
        }
    }

    fn select_upstream(&self) -> Arc<UpstreamNode> {
        match self.strategy {
            UpstreamStrategy::Priority => {
                // Nodes are already pre-sorted by primary priority in `new()`
                self.nodes.first().cloned().unwrap()
            }
            UpstreamStrategy::LoadBalance => {
                // Random round-robin amongst healthy nodes
                let mut rng = rand::thread_rng();
                self.nodes.choose(&mut rng).cloned().unwrap()
            }
            UpstreamStrategy::Fastest => {
                // Pick the node with the lowest recorded latency
                self.nodes.iter().min_by_key(|n| n.last_latency_ms.load(Ordering::Relaxed)).cloned().unwrap()
            }
        }
    }
}
