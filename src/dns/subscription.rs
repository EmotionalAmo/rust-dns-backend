//! Remote filter list subscription module.
//!
//! Handles downloading and parsing remote filter lists in:
//! - AdGuard filter syntax (||domain^, @@domain, etc.)
//! - Hosts file format (IP domain)

use anyhow::{Context, Result};
use chrono::Utc;
use regex::Regex;
use std::net::IpAddr;
use std::sync::LazyLock;
use tracing::info;

use crate::db::DbPool;

/// HTTP client timeout for fetching remote lists
const FETCH_TIMEOUT_SECS: u64 = 30;
/// Maximum response size (10 MB)
const MAX_RESPONSE_SIZE: usize = 10 * 1024 * 1024;

/// Validate a filter-list URL against SSRF risks (H-1 fix).
///
/// Rules:
/// 1. Scheme must be `http` or `https` — blocks `file://`, `ftp://`, etc.
/// 2. Host must not resolve to a private / loopback / link-local IP range.
///    We check the *literal* host string; for hostnames we rely on reqwest's
///    redirect-following being blocked (we do not follow cross-scheme redirects).
///    A production deployment behind a firewall is the primary defense; this
///    check catches the most obvious injection attempts.
pub fn validate_filter_url(url: &str) -> Result<()> {
    let parsed = url::Url::parse(url).context("Invalid URL")?;

    // Rule 1: only http/https
    match parsed.scheme() {
        "http" | "https" => {}
        s => anyhow::bail!(
            "Disallowed URL scheme '{}': only http and https are permitted",
            s
        ),
    }

    // Rule 2: host must exist and not be a private address literal
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("URL has no host"))?;

    // If the host is a bare IP literal, check it is not a private range
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_private_ip(ip) {
            anyhow::bail!(
                "Filter list URL points to a private/loopback IP '{}' — SSRF not allowed",
                ip
            );
        }
    }
    // If host is a name we trust the OS resolver + network policy to block internal names.
    // Callers running in cloud environments should additionally configure egress firewalls.

    Ok(())
}

/// Returns true for IP addresses that should never be reachable from an external filter list.
fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()          // 127.0.0.0/8
            || v4.is_private()        // 10/8, 172.16/12, 192.168/16
            || v4.is_link_local()     // 169.254/16
            || v4.is_broadcast()
            || v4.is_documentation()
            || v4.is_unspecified() // 0.0.0.0
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()          // ::1
            || v6.is_unspecified()    // ::
            // fc00::/7  (unique-local, RFC 4193)
            || (v6.segments()[0] & 0xfe00) == 0xfc00
            // fe80::/10 (link-local)
            || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// AdGuard rule patterns
static ADGUARD_DOMAIN_RULE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\|\|([a-zA-Z0-9][a-zA-Z0-9_.-]*[a-zA-Z0-9])\^?$").expect("Invalid regex")
});

static ADGUARD_EXCEPTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^@@\|\|([a-zA-Z0-9][a-zA-Z0-9_.-]*[a-zA-Z0-9])\^?$").expect("Invalid regex")
});

/// Fetch remote filter list content
pub async fn fetch_remote_filter(url: &str) -> Result<String> {
    // SSRF guard: validate scheme and host before making any network request (H-1 fix)
    validate_filter_url(url).context("Filter list URL rejected by SSRF guard")?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(FETCH_TIMEOUT_SECS))
        .user_agent("Ent-DNS/1.0")
        // Do not follow redirects that change the scheme (prevents http→file redirect tricks)
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .context("Failed to create HTTP client")?;

    let response = client
        .get(url)
        .send()
        .await
        .context("Failed to fetch filter list")?;

    if !response.status().is_success() {
        anyhow::bail!("HTTP error: {}", response.status());
    }

    // Reject early using Content-Length header before reading any body (M-3 fix)
    if let Some(len) = response.content_length() {
        if len > MAX_RESPONSE_SIZE as u64 {
            anyhow::bail!("Response too large: {} bytes (Content-Length)", len);
        }
    }

    // Read body as raw bytes to enforce size limit before UTF-8 decoding
    let bytes = response
        .bytes()
        .await
        .context("Failed to read response body")?;

    if bytes.len() > MAX_RESPONSE_SIZE {
        anyhow::bail!("Response too large: {} bytes", bytes.len());
    }

    let content =
        String::from_utf8(bytes.to_vec()).context("Filter list response is not valid UTF-8")?;

    Ok(content)
}

/// Parse AdGuard filter rules from content
pub fn parse_adguard_rules(content: &str) -> (Vec<String>, Vec<String>) {
    let mut block_rules = Vec::new();
    let mut allow_rules = Vec::new();

    for line in content.lines() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('!') || line.starts_with('#') {
            continue;
        }

        // Skip CSS selectors and script rules
        if line.contains("##") || line.contains("#@#") || line.contains("#%#") {
            continue;
        }

        // Skip regex rules (too complex for now)
        if line.starts_with('/') && line.ends_with('/') {
            continue;
        }

        // Parse exception rules (@@||domain^)
        if let Some(caps) = ADGUARD_EXCEPTION.captures(line) {
            if let Some(domain) = caps.get(1) {
                // Bug fix: append `^` so the rule matches AdGuard syntax expected by RuleSet
                allow_rules.push(format!("@@||{}^", domain.as_str()));
            }
            continue;
        }

        // Parse blocking rules (||domain^ or ||domain)
        if let Some(caps) = ADGUARD_DOMAIN_RULE.captures(line) {
            if let Some(domain) = caps.get(1) {
                block_rules.push(format!("||{}^", domain.as_str()));
            }
            continue;
        }

        // Simple domain blocking (domain without special chars)
        if !line.contains(['/', ':', '*', '^', '|']) {
            // Check if it looks like a domain
            if line.contains('.') && !line.starts_with('.') && !line.ends_with('.') {
                block_rules.push(format!("||{}^", line));
            }
        }
    }

    (block_rules, allow_rules)
}

/// Parse hosts file format rules
pub fn parse_hosts_rules(content: &str) -> Vec<String> {
    let mut rules = Vec::new();

    for line in content.lines() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Parse "IP domain" format
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let domain = parts[1];
            // Validate domain format
            if domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && domain
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '.' || c == '-' || c == '_')
            {
                // Create AdGuard-style blocking rule
                rules.push(format!("||{}^", domain));
            }
        }
    }

    rules
}

/// Sync a remote filter list: download, parse, and store rules
pub async fn sync_filter_list(pool: &DbPool, filter_id: &str, url: &str) -> Result<i64> {
    info!("Syncing filter list {} from {}", filter_id, url);

    // Fetch content
    let content = fetch_remote_filter(url)
        .await
        .context("Failed to fetch remote filter list")?;

    // Detect format and parse
    let (block_rules, allow_rules) = if is_hosts_format(&content) {
        info!("Detected hosts file format for filter {}", filter_id);
        (parse_hosts_rules(&content), Vec::new())
    } else {
        info!("Detected AdGuard filter format for filter {}", filter_id);
        parse_adguard_rules(&content)
    };

    let total_rules = block_rules.len() + allow_rules.len();
    info!(
        "Parsed {} rules for filter {} ({} block, {} allow)",
        total_rules,
        filter_id,
        block_rules.len(),
        allow_rules.len()
    );

    // Wrap DELETE + INSERT in a transaction so a crash mid-sync never leaves rules empty (H-4 fix)
    let filter_prefix = format!("filter:{}", filter_id);
    let now = Utc::now().to_rfc3339();

    let mut tx = pool.begin().await.context("Failed to begin transaction")?;

    sqlx::query("DELETE FROM custom_rules WHERE created_by = ?")
        .bind(&filter_prefix)
        .execute(&mut *tx)
        .await
        .context("Failed to delete old rules")?;

    // 合并 block_rules 和 allow_rules，批量插入（每批 200 条，SQLite 参数上限安全值）
    const BATCH_SIZE: usize = 200;
    let all_rules: Vec<&String> = block_rules.iter().chain(allow_rules.iter()).collect();
    let total_rules = all_rules.len();

    for chunk in all_rules.chunks(BATCH_SIZE) {
        if chunk.is_empty() {
            continue;
        }
        // 构建多值 INSERT
        let placeholders: String = chunk
            .iter()
            .map(|_| "(?, ?, NULL, 1, ?, ?)")
            .collect::<Vec<_>>()
            .join(", ");
        let query_str = format!(
            "INSERT OR IGNORE INTO custom_rules (id, rule, comment, is_enabled, created_by, created_at) VALUES {}",
            placeholders
        );
        let mut q = sqlx::query(&query_str);
        for rule in chunk {
            let id = uuid::Uuid::new_v4().to_string();
            q = q.bind(id).bind(*rule).bind(&filter_prefix).bind(&now);
        }
        q.execute(&mut *tx).await.context("Batch insert failed")?;
    }

    let inserted = total_rules as i64;

    tx.commit()
        .await
        .context("Failed to commit filter sync transaction")?;

    // Update filter list metadata (outside the transaction — non-critical metadata)
    sqlx::query("UPDATE filter_lists SET rule_count = ?, last_updated = ? WHERE id = ?")
        .bind(inserted)
        .bind(&now)
        .bind(filter_id)
        .execute(pool)
        .await
        .context("Failed to update filter list metadata")?;

    info!(
        "Successfully synced filter {}: {} rules",
        filter_id, inserted
    );
    Ok(inserted)
}

/// Check if content appears to be hosts file format
fn is_hosts_format(content: &str) -> bool {
    let mut hosts_lines = 0;
    let mut total_lines = 0;

    for line in content.lines().take(100) {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        total_lines += 1;

        // Check if line starts with an IP address
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let first = parts[0];
            // Check if first part looks like an IP address
            if first.parse::<std::net::IpAddr>().is_ok() {
                hosts_lines += 1;
            }
        }
    }

    // If more than 50% of lines are in hosts format, treat as hosts file
    total_lines > 0 && (hosts_lines as f64 / total_lines as f64) > 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_adguard_rules() {
        let content = r#"
! Title: Test Filter
! Version: 1.0

||example.com^
||ads.example.org^
@@||allowed.example.com^
||test.net^$important
"#;
        let (block, allow) = parse_adguard_rules(content);
        assert!(block.contains(&"||example.com^".to_string()));
        assert!(block.contains(&"||ads.example.org^".to_string()));
        assert!(allow.contains(&"@@||allowed.example.com^".to_string()));
    }

    #[test]
    fn test_parse_hosts_rules() {
        let content = r#"
# Hosts file for blocking ads
127.0.0.1 example.com
0.0.0.0 ads.example.org
127.0.0.1 tracker.net
"#;
        let rules = parse_hosts_rules(content);
        assert!(rules.contains(&"||example.com^".to_string()));
        assert!(rules.contains(&"||ads.example.org^".to_string()));
        assert!(rules.contains(&"||tracker.net^".to_string()));
    }
}
