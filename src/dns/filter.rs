#![allow(dead_code)]

use super::rules::RuleSet;
use crate::db::DbPool;
use anyhow::Result;
use arc_swap::ArcSwap;
use std::collections::HashMap;
use std::sync::Arc;

/// FilterEngine 使用 ArcSwap 实现热路径无锁读取。
///
/// reload() 在锁外构建新的数据结构，然后原子地 store 新 Arc。
/// is_blocked() / check_rewrite() 是纯同步方法，load() 返回一个轻量 Guard，
/// 无需 await，读性能接近裸指针解引用。
pub struct FilterEngine {
    rules: ArcSwap<RuleSet>,
    rewrites: ArcSwap<HashMap<String, String>>,
    db: DbPool,
}

impl FilterEngine {
    pub async fn new(db: DbPool) -> Result<Self> {
        let engine = Self {
            rules: ArcSwap::from_pointee(RuleSet::new()),
            rewrites: ArcSwap::from_pointee(HashMap::new()),
            db,
        };
        engine.reload().await?;
        Ok(engine)
    }

    /// Reload all rules and rewrites from the database.
    /// 在锁外构建新数据结构，然后原子地替换——读路径全程不阻塞。
    pub async fn reload(&self) -> Result<()> {
        // 预估规则数量以便预分配内存
        let expected_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM custom_rules WHERE is_enabled = 1")
                .fetch_one(&self.db)
                .await
                .unwrap_or(0);

        let mut new_rules = RuleSet::with_capacity(expected_count as usize);
        let mut total = 0usize;

        // Load custom rules (AdGuard syntax stored in DB)
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT rule FROM custom_rules WHERE is_enabled = 1")
                .fetch_all(&self.db)
                .await?;

        for (rule,) in rows {
            if new_rules.add_rule(&rule) {
                total += 1;
            }
        }

        // Safety guard: warn if total rules is approaching memory limits
        const MAX_CUSTOM_RULES: usize = 500_000;
        if total > MAX_CUSTOM_RULES {
            tracing::warn!(
                "FilterEngine: custom rule count ({}) exceeds MAX_CUSTOM_RULES ({}). \
                 Consider reducing custom rules or increasing system memory.",
                total,
                MAX_CUSTOM_RULES
            );
        }

        // Load filter list count
        let list_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM filter_lists WHERE is_enabled = 1")
                .fetch_one(&self.db)
                .await?;

        // Load DNS rewrites
        let mut new_rewrites = HashMap::new();
        let rewrite_rows: Vec<(String, String)> =
            sqlx::query_as("SELECT domain, answer FROM dns_rewrites")
                .fetch_all(&self.db)
                .await?;

        for (domain, answer) in rewrite_rows {
            new_rewrites.insert(domain.to_lowercase(), answer);
        }

        // Add Safe Search rewrites if enabled (in-memory only, not persisted to DB)
        let safe_search_enabled: String =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = 'safe_search_enabled'")
                .fetch_one(&self.db)
                .await
                .unwrap_or_else(|_| "false".to_string());

        if safe_search_enabled == "true" {
            tracing::info!("Safe Search is enabled: adding dynamic rewrites");
            // Safe Search redirect rules (RFC 8484 DNS-level enforcement)
            let safe_search_rules = [
                ("www.google.com", "forcesafesearch.google.com"),
                ("google.com", "forcesafesearch.google.com"),
                ("www.bing.com", "strict.bing.com"),
                ("bing.com", "strict.bing.com"),
                ("www.youtube.com", "restrict.youtube.com"),
                ("youtube.com", "restrict.youtube.com"),
                ("duckduckgo.com", "safe.duckduckgo.com"),
                ("yandex.com", "family.yandex.com"),
                ("www.yandex.com", "family.yandex.com"),
            ];
            for (domain, target) in safe_search_rules {
                new_rewrites.insert(domain.to_lowercase(), target.to_string());
            }
        }

        // Add Parental Control rules if enabled
        let parental_enabled: String =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = 'parental_control_enabled'")
                .fetch_one(&self.db)
                .await
                .unwrap_or_else(|_| "false".to_string());

        if parental_enabled == "true" {
            let protection_level: String = sqlx::query_scalar(
                "SELECT value FROM settings WHERE key = 'parental_control_level'",
            )
            .fetch_one(&self.db)
            .await
            .unwrap_or_else(|_| "none".to_string());

            if protection_level != "none" {
                tracing::info!(
                    "Parental Control enabled at level '{}': adding preset rules",
                    protection_level
                );

                // Load preset categories based on protection level
                let category_rows: Vec<(String,)> = sqlx::query_as(
                    "SELECT domains FROM parental_control_categories WHERE level = ?",
                )
                .bind(&protection_level)
                .fetch_all(&self.db)
                .await?;

                let mut pc_rule_count = 0;
                for (domains_json,) in category_rows {
                    // Parse JSON array of domains
                    if let Ok(domains) = serde_json::from_str::<Vec<String>>(&domains_json) {
                        for domain in domains {
                            // Add as block rule (AdGuard syntax: ||domain.com)
                            let block_rule = format!("||{}", domain);
                            if new_rules.add_rule(&block_rule) {
                                pc_rule_count += 1;
                            }
                        }
                    }
                }

                tracing::info!(
                    "Parental Control: loaded {} rules for level '{}'",
                    pc_rule_count,
                    protection_level
                );
            }
        }

        let rewrite_count = new_rewrites.len();

        // Compile all accumulated wildcard patterns into a RegexSet for O(1) per-query matching.
        new_rules.build();

        // 原子替换：读路径全程不阻塞，无需持锁
        self.rules.store(Arc::new(new_rules));
        self.rewrites.store(Arc::new(new_rewrites));

        tracing::info!(
            "Filter engine reloaded: {} custom rules, {} filter lists, {} rewrites",
            total,
            list_count,
            rewrite_count,
        );
        Ok(())
    }

    /// Check if a domain should be blocked.
    /// 同步方法：arc-swap load() 是无锁读，热路径零 await。
    pub fn is_blocked(&self, domain: &str) -> bool {
        let rules = self.rules.load();
        rules.is_blocked(domain)
    }

    /// Check if a domain has a rewrite rule. Returns the target IP if found.
    /// 同步方法：arc-swap load() 是无锁读，热路径零 await。
    pub fn check_rewrite(&self, domain: &str) -> Option<String> {
        let rewrites = self.rewrites.load();
        rewrites.get(&domain.to_lowercase()).cloned()
    }

    /// Add a single rule at runtime (without DB persistence — use API for persistence).
    /// 注意：此方法触发完整 reload 以保持 ArcSwap 语义一致性。
    /// 对于高频调用场景请改用 reload()。
    pub async fn add_rule_live(&self, rule: &str) {
        // 克隆当前规则集，加入新规则，重新编译 wildcard RegexSet，原子替换
        let current = self.rules.load();
        let mut new_rules = (**current).clone();
        new_rules.add_rule(rule);
        new_rules.build();
        self.rules.store(Arc::new(new_rules));
    }

    pub fn stats(&self) -> (usize, usize, usize) {
        let rules = self.rules.load();
        let rewrites = self.rewrites.load();
        (rules.blocked_count(), rules.allowed_count(), rewrites.len())
    }
}
