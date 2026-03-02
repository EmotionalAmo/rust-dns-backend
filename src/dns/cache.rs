use hickory_proto::rr::RecordType;
use moka::future::Cache;
use moka::Expiry;
use std::time::{Duration, Instant};

/// Minimum TTL floor: prevent excessive cache churn for records with very low TTL.
const TTL_MIN_SECS: u64 = 5;
/// Maximum TTL ceiling: cap overly aggressive upstream TTL values.
const TTL_MAX_SECS: u64 = 86_400; // 24 hours
/// Fallback TTL when the upstream response contains no answer records.
const TTL_DEFAULT_SECS: u64 = 300;

/// Cache entry: the serialised DNS wire format paired with its intended TTL.
/// The TTL is embedded in the value so the `Expiry` impl can read it per-entry.
#[derive(Clone)]
pub struct CacheEntry {
    pub data: Vec<u8>,
    pub ttl: Duration,
}

/// Per-entry expiry policy: each entry expires after its own TTL.
struct DnsCacheExpiry;

impl Expiry<String, CacheEntry> for DnsCacheExpiry {
    fn expire_after_create(
        &self,
        _key: &String,
        value: &CacheEntry,
        _created_at: Instant,
    ) -> Option<Duration> {
        Some(value.ttl)
    }
}

pub struct DnsCache {
    inner: Cache<String, CacheEntry>,
}

impl Default for DnsCache {
    fn default() -> Self {
        Self::new()
    }
}

impl DnsCache {
    pub fn new() -> Self {
        let inner = Cache::builder()
            .max_capacity(50_000)
            .expire_after(DnsCacheExpiry)
            .build();
        Self { inner }
    }

    fn cache_key(domain: &str, qtype: RecordType) -> String {
        format!("{}:{:?}", domain.to_lowercase(), qtype)
    }

    pub async fn get(&self, domain: &str, qtype: RecordType) -> Option<Vec<u8>> {
        self.inner
            .get(&Self::cache_key(domain, qtype))
            .await
            .map(|e| e.data)
    }

    /// Store a DNS response with a TTL derived from the upstream answer records.
    ///
    /// `min_ttl` is the minimum TTL (seconds) across all answer records.
    /// Pass `None` to use the default TTL.
    pub async fn set_with_ttl(
        &self,
        domain: &str,
        qtype: RecordType,
        data: Vec<u8>,
        min_ttl: Option<u32>,
    ) {
        let ttl_secs = min_ttl
            .map(|t| t as u64)
            .unwrap_or(TTL_DEFAULT_SECS)
            .clamp(TTL_MIN_SECS, TTL_MAX_SECS);
        let entry = CacheEntry {
            data,
            ttl: Duration::from_secs(ttl_secs),
        };
        self.inner
            .insert(Self::cache_key(domain, qtype), entry)
            .await;
    }

    /// Convenience wrapper using the default TTL (for synthetic/rewrite records).
    pub async fn set(&self, domain: &str, qtype: RecordType, data: Vec<u8>) {
        self.set_with_ttl(domain, qtype, data, None).await;
    }

    /// Returns the number of entries currently in the cache.
    pub fn entry_count(&self) -> u64 {
        self.inner.entry_count()
    }

    /// Invalidates all entries in the cache and waits for pending tasks to complete.
    pub async fn invalidate_all(&self) {
        self.inner.invalidate_all();
        self.inner.run_pending_tasks().await;
    }
}
