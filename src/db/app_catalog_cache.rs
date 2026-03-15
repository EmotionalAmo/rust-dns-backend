use crate::db::DbPool;
use std::collections::HashMap;
use std::sync::RwLock;
use tracing::{info, warn};

/// In-memory cache for mapping DNS query domains to App IDs.
/// This completely eliminates the severe N+1 `LIKE '%.' || ad.domain`
/// performance bottleneck during high-volume query log insertion.
pub struct AppCatalogCache {
    /// Maps a raw domain string (e.g., "youtube.com") to its App ID.
    domains_to_app_id: RwLock<HashMap<String, i32>>,
}

impl Default for AppCatalogCache {
    fn default() -> Self {
        Self::new()
    }
}

impl AppCatalogCache {
    /// Create a new, empty cache.
    pub fn new() -> Self {
        Self {
            domains_to_app_id: RwLock::new(HashMap::new()),
        }
    }

    /// Load the domain-to-app mappings from the database.
    pub async fn load_from_db(&self, db: &DbPool) {
        let rows: Result<Vec<(i32, String)>, sqlx::Error> =
            sqlx::query_as("SELECT app_id, domain FROM app_domains")
                .fetch_all(db)
                .await;

        match rows {
            Ok(mapped_domains) => {
                let mut cache = self.domains_to_app_id.write().unwrap();
                cache.clear();
                let count = mapped_domains.len();
                for (app_id, domain) in mapped_domains {
                    cache.insert(domain, app_id);
                }
                info!("AppCatalogCache loaded {} domain mappings", count);
            }
            Err(e) => {
                warn!("Failed to load app_domains into memory: {}", e);
            }
        }
    }

    /// Match a domain against the loaded catalog.
    /// Checks exact match first, then checks successive parent domains
    /// (e.g., sub.test.example.com -> test.example.com -> example.com)
    pub fn match_domain(&self, domain: &str) -> Option<i32> {
        let cache = self.domains_to_app_id.read().unwrap();
        if cache.is_empty() {
            return None;
        }

        // Clean trailing dot if present
        let clean_domain = domain.trim_end_matches('.');

        // Exact match
        if let Some(&app_id) = cache.get(clean_domain) {
            return Some(app_id);
        }

        // Suffix match (e.g., api.youtube.com -> check youtube.com)
        let mut parts: Vec<&str> = clean_domain.split('.').collect();
        while parts.len() > 1 {
            parts.remove(0); // slice off the leading subdomain
            let parent = parts.join(".");
            if let Some(&app_id) = cache.get(&parent) {
                return Some(app_id);
            }
        }

        None
    }
}
