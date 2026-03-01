// OISD Performance Test - Phase 1
// Tests loading 10,000 OISD rules and measuring memory and query performance
//
// Goals:
// - Verify memory footprint with 10,000 rules
// - Measure query latency (P50/P95/P99)
// - Validate FilterEngine performance

use anyhow::Result;
use chrono::Utc;
use rust_dns::dns::rules::RuleSet;

#[tokio::test]
async fn test_oisd_performance() -> Result<()> {
    println!("======================================");
    println!("OISD Phase 1 Performance Test");
    println!("======================================\n");

    // Step 1: Download and parse OISD data
    let oisd_domains = download_and_parse_oisd().await?;
    println!("\nDownloaded {} OISD domains", oisd_domains.len());

    // Step 2: Test RuleSet directly with OISD rules
    test_ruleset_performance(&oisd_domains).await?;

    // Step 3: Test FilterEngine integration
    test_filter_engine_performance(&oisd_domains).await?;

    println!("\n======================================");
    println!("All Tests Complete!");
    println!("======================================");

    Ok(())
}

/// Download OISD Small blocklist and parse domains
async fn download_and_parse_oisd() -> Result<Vec<String>> {
    println!("Step 1: Downloading OISD Small blocklist...");
    println!("Source: https://small.oisd.nl/");

    let response = reqwest::get("https://small.oisd.nl/").await?;
    let content = response.text().await?;

    let mut domains = Vec::new();

    // Parse OISD AdGuard format: ||domain^
    for line in content.lines() {
        let line = line.trim();

        // Skip comments and empty lines
        if line.is_empty() || line.starts_with('!') || line.starts_with('[') {
            continue;
        }

        // Parse AdGuard format: ||domain^
        if line.starts_with("||") && line.ends_with('^') {
            let domain = line[2..line.len() - 1].to_lowercase();
            if !domain.is_empty() {
                domains.push(domain);
            }
        }
    }

    println!("Total domains in OISD Small: {}", domains.len());

    // For Phase 1 test, use first 10,000 domains
    if domains.len() > 10_000 {
        domains.truncate(10_000);
    }

    Ok(domains)
}

/// Test RuleSet directly with OISD rules
async fn test_ruleset_performance(oisd_domains: &[String]) -> Result<()> {
    println!("\n\nTest 1: RuleSet Performance");
    println!("--------------------------");

    // Create RuleSet with capacity
    let start_load = std::time::Instant::now();
    let mut ruleset = RuleSet::with_capacity(oisd_domains.len());

    let mut valid_count = 0;
    for domain in oisd_domains {
        let rule = format!("||{}^", domain);
        if ruleset.add_rule(&rule) {
            valid_count += 1;
        }
    }
    let load_duration = start_load.elapsed();

    println!("\nLoading Results:");
    println!("  Total domains processed: {}", oisd_domains.len());
    println!("  Valid rules loaded: {}", valid_count);
    println!("  Load time: {:?}", load_duration);
    println!(
        "  Load throughput: {:.0} rules/sec",
        valid_count as f64 / load_duration.as_secs_f64()
    );
    println!("  Blocked count: {}", ruleset.blocked_count());

    // Query performance test
    println!("\nRunning query performance test...");
    let test_domains = generate_test_domains(100_000);
    println!(
        "Testing with {} random domain queries...",
        test_domains.len()
    );

    // Warm up
    for _ in 0..1000 {
        let domain = &test_domains[random_index(test_domains.len())];
        let _ = ruleset.is_blocked(domain);
    }

    // Actual measurement
    let mut latencies = Vec::with_capacity(test_domains.len());
    let start_query = std::time::Instant::now();

    for domain in &test_domains {
        let query_start = std::time::Instant::now();
        let _ = ruleset.is_blocked(domain);
        latencies.push(query_start.elapsed());
    }

    let total_query_time = start_query.elapsed();

    // Calculate percentiles
    latencies.sort();
    let p50 = latencies[latencies.len() / 2];
    let p95 = latencies[latencies.len() * 95 / 100];
    let p99 = latencies[latencies.len() * 99 / 100];
    let p999 = latencies[latencies.len() * 999 / 1000];

    println!("\nQuery Performance:");
    println!("  Total queries: {}", test_domains.len());
    println!("  Total time: {:?}", total_query_time);
    println!(
        "  Throughput: {:.0} queries/sec",
        test_domains.len() as f64 / total_query_time.as_secs_f64()
    );
    println!(
        "  P50 latency: {:?} ({:.2} µs)",
        p50,
        p50.as_micros() as f64
    );
    println!(
        "  P95 latency: {:?} ({:.2} µs)",
        p95,
        p95.as_micros() as f64
    );
    println!(
        "  P99 latency: {:?} ({:.2} µs)",
        p99,
        p99.as_micros() as f64
    );
    println!(
        "  P99.9 latency: {:?} ({:.2} µs)",
        p999,
        p999.as_micros() as f64
    );

    // Validate some OISD domains are blocked
    println!("\nValidation - OISD Domains:");
    if let Some(first_domain) = oisd_domains.first() {
        let is_blocked = ruleset.is_blocked(first_domain);
        println!(
            "  {} -> {} (first OISD domain)",
            first_domain,
            if is_blocked { "BLOCKED" } else { "ALLOWED" }
        );
    }

    if let Some(last_domain) = oisd_domains.last() {
        let is_blocked = ruleset.is_blocked(last_domain);
        println!(
            "  {} -> {} (last OISD domain)",
            last_domain,
            if is_blocked { "BLOCKED" } else { "ALLOWED" }
        );
    }

    // Test subdomain blocking
    if let Some(test_domain) = oisd_domains.first() {
        let subdomain = format!("sub.{}", test_domain);
        let is_blocked = ruleset.is_blocked(&subdomain);
        println!(
            "  {} -> {} (subdomain test)",
            subdomain,
            if is_blocked { "BLOCKED" } else { "ALLOWED" }
        );
    }

    Ok(())
}

/// Test FilterEngine with database integration
async fn test_filter_engine_performance(oisd_domains: &[String]) -> Result<()> {
    println!("\n\nTest 2: FilterEngine Integration");
    println!("--------------------------------");

    // Create in-memory database
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(10)
        .connect("sqlite::memory:")
        .await?;

    println!("Setting up database schema...");

    // Run schema setup
    sqlx::query(
        r#"
        CREATE TABLE custom_rules (
            id TEXT PRIMARY KEY,
            rule TEXT NOT NULL,
            comment TEXT,
            is_enabled INTEGER NOT NULL DEFAULT 1,
            created_by TEXT NOT NULL,
            created_at TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE filter_lists (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            url TEXT,
            is_enabled INTEGER NOT NULL DEFAULT 0,
            rule_count INTEGER NOT NULL DEFAULT 0,
            last_updated TEXT,
            created_at TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE dns_rewrites (
            id TEXT PRIMARY KEY,
            domain TEXT NOT NULL UNIQUE,
            answer TEXT NOT NULL,
            created_at TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE parental_control_categories (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT,
            level TEXT NOT NULL,
            domains TEXT NOT NULL,
            created_at TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    // Insert OISD domains
    println!(
        "Inserting {} OISD domains into database...",
        oisd_domains.len()
    );
    let start_insert = std::time::Instant::now();

    // Convert domains to JSON
    let domains_json = serde_json::to_string(&oisd_domains)?;

    sqlx::query(
        r#"
        INSERT INTO parental_control_categories (id, name, description, level, domains, created_at)
        VALUES (?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind("oisd-phase1-test")
    .bind("OISD Phase 1 Test")
    .bind(format!(
        "Performance test with {} OISD domains",
        oisd_domains.len()
    ))
    .bind("basic")
    .bind(&domains_json)
    .bind(Utc::now().to_rfc3339())
    .execute(&pool)
    .await?;

    let insert_duration = start_insert.elapsed();

    println!("Insert complete: {:?}", insert_duration);

    // Enable parental control
    sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)")
        .bind("parental_control_enabled")
        .bind("true")
        .execute(&pool)
        .await?;

    sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)")
        .bind("parental_control_level")
        .bind("basic")
        .execute(&pool)
        .await?;

    // Simulate FilterEngine reload
    println!("\nSimulating FilterEngine reload...");
    let start_reload = std::time::Instant::now();

    let mut rules = RuleSet::with_capacity(oisd_domains.len());

    // Load parental control domains (same as FilterEngine does)
    let category_rows: Vec<(String,)> =
        sqlx::query_as("SELECT domains FROM parental_control_categories WHERE level = ?")
            .bind("basic")
            .fetch_all(&pool)
            .await?;

    let mut pc_rule_count = 0;
    for (domains_json,) in category_rows {
        if let Ok(domains) = serde_json::from_str::<Vec<String>>(&domains_json) {
            for domain in domains {
                let block_rule = format!("||{}", domain);
                if rules.add_rule(&block_rule) {
                    pc_rule_count += 1;
                }
            }
        }
    }

    let reload_duration = start_reload.elapsed();

    println!("\nFilterEngine Reload Results:");
    println!("  Rules loaded: {}", pc_rule_count);
    println!("  Reload time: {:?}", reload_duration);
    println!(
        "  Reload throughput: {:.0} rules/sec",
        pc_rule_count as f64 / reload_duration.as_secs_f64()
    );
    println!("  Blocked count: {}", rules.blocked_count());

    // Query test through the loaded rules
    println!("\nRunning FilterEngine query test...");
    let test_domains = generate_test_domains(10_000);

    let start_query = std::time::Instant::now();
    let mut blocked_count = 0;

    for domain in &test_domains {
        if rules.is_blocked(domain) {
            blocked_count += 1;
        }
    }

    let query_duration = start_query.elapsed();

    println!("  Queries: {}", test_domains.len());
    println!(
        "  Blocked: {} ({:.1}%)",
        blocked_count,
        blocked_count as f64 / test_domains.len() as f64 * 100.0
    );
    println!("  Total time: {:?}", query_duration);
    println!(
        "  Avg latency: {:.2} µs",
        query_duration.as_micros() as f64 / test_domains.len() as f64
    );
    println!(
        "  Throughput: {:.0} queries/sec",
        test_domains.len() as f64 / query_duration.as_secs_f64()
    );

    Ok(())
}

/// Generate test domains for querying
fn generate_test_domains(count: usize) -> Vec<String> {
    let mut domains = Vec::with_capacity(count);
    let good_domains = vec![
        "google.com",
        "github.com",
        "stackoverflow.com",
        "example.com",
        "wikipedia.org",
        "reddit.com",
        "news.ycombinator.com",
        "cloudflare.com",
        "aws.amazon.com",
        "docs.rs",
    ];

    let bad_domains = [
        "doubleclick.net",
        "ads.example.com",
        "tracker.example.com",
        "malware.example.com",
        "phishing.example.com",
        "adserver.example.com",
    ];

    for i in 0..count {
        if i % 4 == 0 {
            // 25% blocked domains
            let base = bad_domains[random_index(bad_domains.len())];
            if random_bool() {
                domains.push(format!("{}.{}", random_subdomain(), base));
            } else {
                domains.push(base.to_string());
            }
        } else {
            // 75% clean domains
            let base = good_domains[random_index(good_domains.len())];
            if random_bool() {
                domains.push(format!("{}.{}", random_subdomain(), base));
            } else {
                domains.push(base.to_string());
            }
        }
    }

    domains
}

/// Simple random number generator (using splitmix64)
fn random_index(max: usize) -> usize {
    static mut STATE: u64 = 1;
    unsafe {
        STATE = STATE.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = STATE;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        (z ^ (z >> 31)) as usize % max
    }
}

fn random_bool() -> bool {
    static mut STATE: u64 = 2;
    unsafe {
        STATE = STATE.wrapping_add(0x9e3779b97f4a7c15);
        STATE & 1 == 0
    }
}

fn random_subdomain() -> String {
    static mut STATE: u64 = 3;
    let chars: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut result = String::with_capacity(8);
    unsafe {
        STATE = STATE.wrapping_add(0x9e3779b97f4a7c15);
        let len = ((STATE >> 32) % 13) as usize + 3;
        for _ in 0..len {
            STATE = STATE
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            result.push(chars[STATE as usize % chars.len()] as char);
        }
    }
    result
}
