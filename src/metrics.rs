use std::sync::atomic::{AtomicU64, Ordering};

/// Global DNS query counters shared between the DNS server and the API.
#[derive(Default)]
pub struct DnsMetrics {
    pub queries_total: AtomicU64,
    pub queries_blocked: AtomicU64,
    pub queries_allowed: AtomicU64,
    pub queries_cached: AtomicU64,
}

impl DnsMetrics {
    pub fn inc_blocked(&self) {
        self.queries_total.fetch_add(1, Ordering::Relaxed);
        self.queries_blocked.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_allowed(&self) {
        self.queries_total.fetch_add(1, Ordering::Relaxed);
        self.queries_allowed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_cached(&self) {
        self.queries_total.fetch_add(1, Ordering::Relaxed);
        self.queries_cached.fetch_add(1, Ordering::Relaxed);
    }

    /// Serialize to Prometheus text exposition format.
    pub fn to_prometheus_text(&self) -> String {
        let total = self.queries_total.load(Ordering::Relaxed);
        let blocked = self.queries_blocked.load(Ordering::Relaxed);
        let allowed = self.queries_allowed.load(Ordering::Relaxed);
        let cached = self.queries_cached.load(Ordering::Relaxed);

        format!(
            "# HELP ent_dns_queries_total Total DNS queries processed\n\
             # TYPE ent_dns_queries_total counter\n\
             ent_dns_queries_total{{status=\"blocked\"}} {blocked}\n\
             ent_dns_queries_total{{status=\"allowed\"}} {allowed}\n\
             ent_dns_queries_total{{status=\"cached\"}} {cached}\n\
             ent_dns_queries_total{{status=\"total\"}} {total}\n"
        )
    }
}
