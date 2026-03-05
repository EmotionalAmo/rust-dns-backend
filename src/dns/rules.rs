//! AdGuard DNS filtering rule parser.
//!
//! Supported syntax:
//!   `||example.com^`          — block domain and all subdomains
//!   `@@||example.com^`        — allowlist domain and all subdomains
//!   `0.0.0.0 example.com`     — hosts-format block
//!   `127.0.0.1 example.com`   — hosts-format redirect (treated as block for now)
//!   `example.com`             — plain domain block (exact + subdomains)
//!   `*.example.com`           — wildcard subdomain block
//!   `||ad-*.example.com^`     — intra-label wildcard block (regex-backed)
//!   `# comment` / `! comment` — ignored
#![allow(dead_code)]

use ahash::AHashSet;
use regex::RegexSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchResult {
    /// Domain is explicitly allowed by an allowlist rule
    Allowed,
    /// Domain is explicitly blocked by a blocklist rule
    Blocked,
    /// Domain did not match any rules
    None,
}

#[derive(Debug, Clone)]
pub struct RuleSet {
    /// Domains in block list. A match blocks `domain` and all its subdomains.
    blocked: AHashSet<Box<str>>,
    /// Domains in allow list. Allow overrides block.
    allowed: AHashSet<Box<str>>,
    /// Raw regex patterns for wildcard block rules, accumulated during build phase.
    wildcard_blocked_patterns: Vec<String>,
    /// Raw regex patterns for wildcard allow rules, accumulated during build phase.
    wildcard_allowed_patterns: Vec<String>,
    /// Compiled RegexSet for wildcard block matching (built by `build()`).
    wildcard_blocked_set: Option<RegexSet>,
    /// Compiled RegexSet for wildcard allow matching (built by `build()`).
    wildcard_allowed_set: Option<RegexSet>,
}

impl Default for RuleSet {
    fn default() -> Self {
        Self::new()
    }
}

impl RuleSet {
    pub fn new() -> Self {
        Self {
            blocked: AHashSet::new(),
            allowed: AHashSet::new(),
            wildcard_blocked_patterns: Vec::new(),
            wildcard_allowed_patterns: Vec::new(),
            wildcard_blocked_set: None,
            wildcard_allowed_set: None,
        }
    }

    pub fn with_capacity(n: usize) -> Self {
        Self {
            blocked: AHashSet::with_capacity(n),
            allowed: AHashSet::with_capacity(n / 10 + 8),
            wildcard_blocked_patterns: Vec::new(),
            wildcard_allowed_patterns: Vec::new(),
            wildcard_blocked_set: None,
            wildcard_allowed_set: None,
        }
    }

    /// Compile all accumulated wildcard patterns into `RegexSet`s for fast matching.
    ///
    /// Call this once after all `add_rule()` calls are complete (before storing in ArcSwap).
    /// Subsequent `add_rule()` calls after `build()` will clear the compiled sets;
    /// call `build()` again to recompile.
    pub fn build(&mut self) {
        self.wildcard_blocked_set = if self.wildcard_blocked_patterns.is_empty() {
            None
        } else {
            RegexSet::new(&self.wildcard_blocked_patterns).ok()
        };
        self.wildcard_allowed_set = if self.wildcard_allowed_patterns.is_empty() {
            None
        } else {
            RegexSet::new(&self.wildcard_allowed_patterns).ok()
        };
    }

    /// Parse a single rule line and add it to the set.
    /// Returns `true` if the line was a valid rule (not a comment or blank).
    pub fn add_rule(&mut self, line: &str) -> bool {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
            return false;
        }

        // Regex rules — skip for now (too complex, rare in DNS context)
        if line.starts_with('/') && line.ends_with('/') {
            return false;
        }

        // Allowlist: @@||domain^ or @@domain
        if let Some(rest) = line.strip_prefix("@@") {
            if let Some(domain) = parse_adguard_domain(rest) {
                self.allowed.insert(domain.into_boxed_str());
                return true;
            }
            // Try wildcard allowlist
            if let Some(pattern) = parse_adguard_wildcard_pattern(rest) {
                self.wildcard_allowed_patterns.push(pattern);
                self.wildcard_allowed_set = None; // invalidate compiled set
                return true;
            }
            return false;
        }

        // AdGuard format: ||domain^  or ||domain^$options  or ||domain$options
        if let Some(domain) = parse_adguard_domain(line) {
            self.blocked.insert(domain.into_boxed_str());
            return true;
        }

        // AdGuard wildcard format: ||ad-*.example.com^
        if let Some(pattern) = parse_adguard_wildcard_pattern(line) {
            self.wildcard_blocked_patterns.push(pattern);
            self.wildcard_blocked_set = None; // invalidate compiled set
            return true;
        }

        // .domain.com^ or .domain.com (AdGuard subdomain-only rule → DNS-level full block)
        if let Some(inner) = line.strip_prefix('.') {
            let domain_str = inner
                .split('^')
                .next()
                .unwrap_or(inner)
                .split('$')
                .next()
                .unwrap_or(inner)
                .trim_end_matches('.');
            let domain = normalize_domain(domain_str);
            if is_valid_domain(&domain) {
                self.blocked.insert(domain.into_boxed_str());
                return true;
            }
            return false;
        }

        // Hosts format: "0.0.0.0 domain" or "127.0.0.1 domain"
        if let Some(domain) = parse_hosts_line(line) {
            self.blocked.insert(domain.into_boxed_str());
            return true;
        }

        // Wildcard: *.example.com  → block subdomains of example.com
        if let Some(rest) = line.strip_prefix("*.") {
            let domain = normalize_domain(rest);
            if !domain.is_empty() {
                self.blocked.insert(domain.into_boxed_str());
                return true;
            }
        }

        // Plain domain: example.com
        let domain = normalize_domain(line);
        if is_valid_domain(&domain) {
            self.blocked.insert(domain.into_boxed_str());
            return true;
        }

        false
    }

    /// Parse all rules from a multi-line string. Returns count of valid rules added.
    pub fn add_rules_from_str(&mut self, content: &str) -> usize {
        content.lines().filter(|line| self.add_rule(line)).count()
    }

    /// Check a domain against the rules, returning the detailed MatchResult.
    pub fn match_domain(&self, domain: &str) -> MatchResult {
        let domain = domain.trim_end_matches('.').to_lowercase();

        // Check allowlist first — any parent match exempts the domain
        if self.matches_set(&domain, &self.allowed) {
            return MatchResult::Allowed;
        }
        if self.matches_wildcard_set(&domain, &self.wildcard_allowed_set) {
            return MatchResult::Allowed;
        }

        // Check blocklist
        if self.matches_set(&domain, &self.blocked) {
            return MatchResult::Blocked;
        }
        if self.matches_wildcard_set(&domain, &self.wildcard_blocked_set) {
            return MatchResult::Blocked;
        }

        MatchResult::None
    }

    /// Helper wrapper for backward compatibility.
    pub fn is_blocked(&self, domain: &str) -> bool {
        self.match_domain(domain) == MatchResult::Blocked
    }

    /// Returns true if `domain` or any of its parent domains is in `set`.
    fn matches_set(&self, domain: &str, set: &AHashSet<Box<str>>) -> bool {
        // Walk from most-specific to least-specific
        let mut current = domain;
        loop {
            if set.contains(current) {
                return true;
            }
            // Move to parent domain
            match current.find('.') {
                Some(pos) => current = &current[pos + 1..],
                None => return false,
            }
        }
    }

    /// Returns true if `domain` or any of its parent domains matches any pattern in `set`.
    ///
    /// Walks up the domain hierarchy (sub.example.com → example.com → com) until a match
    /// is found or all components are exhausted. Uses `RegexSet::is_match()` which runs
    /// all patterns in a single O(chars) pass — far faster than iterating `Vec<Regex>`.
    fn matches_wildcard_set(&self, domain: &str, set: &Option<RegexSet>) -> bool {
        let set = match set {
            Some(s) if !s.is_empty() => s,
            _ => return false,
        };
        let mut current = domain;
        loop {
            if set.is_match(current) {
                return true;
            }
            match current.find('.') {
                Some(pos) => current = &current[pos + 1..],
                None => return false,
            }
        }
    }

    pub fn blocked_count(&self) -> usize {
        self.blocked.len() + self.wildcard_blocked_patterns.len()
    }

    pub fn allowed_count(&self) -> usize {
        self.allowed.len() + self.wildcard_allowed_patterns.len()
    }

    pub fn wildcard_blocked_count(&self) -> usize {
        self.wildcard_blocked_patterns.len()
    }

    pub fn wildcard_allowed_count(&self) -> usize {
        self.wildcard_allowed_patterns.len()
    }
}

/// Parse `||domain^`, `||domain^$options`, `||domain$options`, `|domain|`, `||domain`
fn parse_adguard_domain(rule: &str) -> Option<String> {
    let rest = if let Some(s) = rule.strip_prefix("||") {
        s
    } else if let Some(s) = rule.strip_prefix('|') {
        s
    } else {
        return None;
    };

    // Strip options: `^` is the canonical separator; `$` is used when `^` is absent
    // e.g. ||domain^$third-party, ||domain$important, ||domain^
    let after_caret = rest.split('^').next().unwrap_or(rest);
    let domain_raw = after_caret.split('$').next().unwrap_or(after_caret);
    let domain = domain_raw
        .trim_end_matches('|')
        .trim_end_matches('/')
        .trim_end_matches('.');

    let domain = normalize_domain(domain);
    if is_valid_domain(&domain) {
        Some(domain)
    } else {
        None
    }
}

/// Parse `||ad-*.example.com^` style rules with intra-label wildcards.
/// Returns the raw regex pattern string if the rule is valid; the caller accumulates
/// these into a `RegexSet` via `RuleSet::build()` for efficient batch matching.
fn parse_adguard_wildcard_pattern(rule: &str) -> Option<String> {
    // Only handle `||` prefix for wildcard rules
    let rest = rule.strip_prefix("||")?;

    // Strip trailing options (^, $options)
    let after_caret = rest.split('^').next().unwrap_or(rest);
    let domain_raw = after_caret.split('$').next().unwrap_or(after_caret);
    let domain_str = domain_raw
        .trim_end_matches('|')
        .trim_end_matches('/')
        .trim_end_matches('.')
        .to_lowercase();

    // Only handle rules that actually contain a wildcard
    if !domain_str.contains('*') {
        return None;
    }

    // Must contain at least one dot (not a bare TLD wildcard)
    if !domain_str.contains('.') {
        return None;
    }

    Some(wildcard_to_regex(&domain_str))
}

/// Convert a wildcard domain pattern to a regex string.
/// `*` matches one or more non-dot characters (intra-label only).
/// Example: `ad-*.aliyuncs.com` → `(?i)^ad\-[^.]+\.aliyuncs\.com$`
fn wildcard_to_regex(pattern: &str) -> String {
    let mut re = String::with_capacity(pattern.len() + 16);
    re.push_str("(?i)^");
    for ch in pattern.chars() {
        match ch {
            '*' => re.push_str("[^.]+"),
            '.' => re.push_str("\\."),
            // Alphanumeric and hyphen/underscore are safe as-is
            c if c.is_alphanumeric() || c == '-' || c == '_' => re.push(c),
            // Escape anything else
            c => {
                re.push('\\');
                re.push(c);
            }
        }
    }
    re.push('$');
    re
}

/// Parse hosts-format line: `0.0.0.0 domain` or `127.0.0.1 domain` or `::1 domain`
fn parse_hosts_line(line: &str) -> Option<String> {
    let mut parts = line.split_whitespace();
    let ip = parts.next()?;

    // Must start with an IP-like string
    let is_ip = ip.parse::<std::net::IpAddr>().is_ok();
    if !is_ip {
        return None;
    }

    let domain_part = parts.next()?;
    // Skip localhost entries
    if domain_part == "localhost" || domain_part.ends_with(".local") {
        return None;
    }

    let domain = normalize_domain(domain_part);
    if is_valid_domain(&domain) {
        Some(domain)
    } else {
        None
    }
}

fn normalize_domain(s: &str) -> String {
    s.trim().trim_end_matches('.').to_lowercase()
}

fn is_valid_domain(s: &str) -> bool {
    if s.is_empty() || s.len() > 253 {
        return false;
    }
    // Must contain at least one dot (not a bare TLD) or be localhost
    if !s.contains('.') && s != "localhost" {
        return false;
    }
    // Basic label validation
    s.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label.chars().all(|c| c.is_alphanumeric() || c == '-')
            && !label.starts_with('-')
            && !label.ends_with('-')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adguard_block_format() {
        let mut rs = RuleSet::new();
        rs.add_rule("||ads.example.com^");
        assert!(rs.is_blocked("ads.example.com"));
        assert!(rs.is_blocked("sub.ads.example.com"));
        assert!(!rs.is_blocked("example.com"));
    }

    #[test]
    fn test_allowlist_overrides_block() {
        let mut rs = RuleSet::new();
        rs.add_rule("||example.com^");
        rs.add_rule("@@||safe.example.com^");
        assert!(rs.is_blocked("ads.example.com"));
        assert!(!rs.is_blocked("safe.example.com"));
        assert!(!rs.is_blocked("sub.safe.example.com"));
    }

    #[test]
    fn test_hosts_format() {
        let mut rs = RuleSet::new();
        rs.add_rule("0.0.0.0 tracker.com");
        rs.add_rule("127.0.0.1 malware.net");
        assert!(rs.is_blocked("tracker.com"));
        assert!(rs.is_blocked("sub.tracker.com"));
        assert!(rs.is_blocked("malware.net"));
    }

    #[test]
    fn test_plain_domain() {
        let mut rs = RuleSet::new();
        rs.add_rule("doubleclick.net");
        assert!(rs.is_blocked("doubleclick.net"));
        assert!(rs.is_blocked("ad.doubleclick.net"));
        assert!(!rs.is_blocked("notdoubleclick.net"));
    }

    #[test]
    fn test_comments_ignored() {
        let mut rs = RuleSet::new();
        assert!(!rs.add_rule("# this is a comment"));
        assert!(!rs.add_rule("! this is also a comment"));
        assert!(!rs.add_rule(""));
        assert_eq!(rs.blocked_count(), 0);
    }

    #[test]
    fn test_wildcard() {
        let mut rs = RuleSet::new();
        rs.add_rule("*.ads.com");
        assert!(rs.is_blocked("sub.ads.com"));
        assert!(rs.is_blocked("deep.sub.ads.com"));
    }

    #[test]
    fn test_fqdn_trailing_dot() {
        let mut rs = RuleSet::new();
        rs.add_rule("||example.com^");
        // DNS queries often come with trailing dot
        assert!(rs.is_blocked("example.com."));
    }

    // --- 新增扩展测试用例 ---

    #[test]
    fn test_case_insensitive_blocking() {
        let mut rs = RuleSet::new();
        rs.add_rule("||Example.COM^");
        // 规则本身应被标准化为小写
        assert!(rs.is_blocked("example.com"));
        assert!(rs.is_blocked("EXAMPLE.COM"));
        assert!(rs.is_blocked("sub.Example.Com"));
    }

    #[test]
    fn test_add_rules_from_str_bulk() {
        let mut rs = RuleSet::new();
        let content =
            "||ads.example.com^\n# comment\n||tracker.net^\n\n! another comment\n||malware.io^";
        let count = rs.add_rules_from_str(content);
        // 3 valid rules, 2 comments, 1 blank = 3 added
        assert_eq!(count, 3);
        assert_eq!(rs.blocked_count(), 3);
        assert!(rs.is_blocked("ads.example.com"));
        assert!(rs.is_blocked("tracker.net"));
        assert!(rs.is_blocked("malware.io"));
    }

    #[test]
    fn test_parent_domain_not_blocked_by_subdomain_rule() {
        // ||sub.example.com^ should NOT block example.com itself
        let mut rs = RuleSet::new();
        rs.add_rule("||sub.example.com^");
        assert!(rs.is_blocked("sub.example.com"));
        assert!(rs.is_blocked("deep.sub.example.com"));
        assert!(!rs.is_blocked("example.com")); // 父域名不应被阻止
        assert!(!rs.is_blocked("other.example.com")); // 兄弟子域名不应被阻止
    }

    #[test]
    fn test_allowlist_only_no_block() {
        // 只有白名单，没有黑名单时，域名不应被阻止
        let mut rs = RuleSet::new();
        rs.add_rule("@@||safe.com^");
        assert!(!rs.is_blocked("safe.com"));
        assert!(!rs.is_blocked("any.domain.com"));
    }

    #[test]
    fn test_hosts_localhost_skipped() {
        // localhost 和 .local 条目应被跳过
        let mut rs = RuleSet::new();
        assert!(!rs.add_rule("127.0.0.1 localhost"));
        assert!(!rs.add_rule("0.0.0.0 mydevice.local"));
        assert_eq!(rs.blocked_count(), 0);
    }

    #[test]
    fn test_regex_rules_skipped() {
        // 正则规则（/pattern/）应被跳过不处理
        let mut rs = RuleSet::new();
        assert!(!rs.add_rule("/^ads\\./"));
        assert_eq!(rs.blocked_count(), 0);
    }

    #[test]
    fn test_bare_tld_rejected() {
        // 裸 TLD（如 "com"）不是有效域名，应被拒绝
        let mut rs = RuleSet::new();
        assert!(!rs.add_rule("com"));
        assert_eq!(rs.blocked_count(), 0);
    }

    #[test]
    fn test_adguard_format_with_options() {
        // ||domain^$third-party 类选项应被忽略，域名正常添加
        let mut rs = RuleSet::new();
        rs.add_rule("||ads.example.com^$third-party");
        assert!(rs.is_blocked("ads.example.com"));
    }

    #[test]
    fn test_stats_after_bulk_add() {
        let mut rs = RuleSet::new();
        rs.add_rule("||block1.com^");
        rs.add_rule("||block2.com^");
        rs.add_rule("@@||allow1.com^");
        assert_eq!(rs.blocked_count(), 2);
        assert_eq!(rs.allowed_count(), 1);
    }

    #[test]
    fn test_ipv6_hosts_format() {
        // ::1 格式的 hosts 条目也应被识别
        let mut rs = RuleSet::new();
        rs.add_rule("::1 ipv6block.com");
        // ipv6block.com 不是 localhost 也不是 .local，应被阻止
        assert!(rs.is_blocked("ipv6block.com"));
    }

    #[test]
    fn test_deep_subdomain_matching() {
        // 深层子域名也应被匹配
        let mut rs = RuleSet::new();
        rs.add_rule("||evil.com^");
        assert!(rs.is_blocked("a.b.c.d.evil.com"));
        assert!(!rs.is_blocked("notevil.com"));
    }

    #[test]
    fn test_dollar_option_without_caret() {
        // ||domain$important 格式（无 ^ 分隔符），应正常解析
        let mut rs = RuleSet::new();
        rs.add_rule("||deloton.com$important");
        assert!(rs.is_blocked("deloton.com"));
        assert!(rs.is_blocked("sub.deloton.com"));
    }

    #[test]
    fn test_dollar_option_with_caret() {
        // ||domain^$third-party 格式，应正常解析
        let mut rs = RuleSet::new();
        rs.add_rule("||tracker.net^$third-party");
        assert!(rs.is_blocked("tracker.net"));
    }

    #[test]
    fn test_leading_dot_rule() {
        // .domain.com^ 格式（AdGuard 子域名规则），在 DNS 层当作全域名阻止
        let mut rs = RuleSet::new();
        rs.add_rule(".bbelements.com^");
        assert!(rs.is_blocked("bbelements.com"));
        assert!(rs.is_blocked("sub.bbelements.com"));
    }

    #[test]
    fn test_leading_dot_rule_no_caret() {
        // .domain.com 格式
        let mut rs = RuleSet::new();
        rs.add_rule(".doublepimp.com");
        assert!(rs.is_blocked("doublepimp.com"));
    }

    // --- 通配符规则测试 ---

    #[test]
    fn test_wildcard_adguard_format() {
        let mut rs = RuleSet::new();
        rs.add_rule("||ad-*.aliyuncs.com^");
        rs.build();
        assert!(rs.is_blocked("ad-foo.aliyuncs.com"));
        assert!(rs.is_blocked("ad-bar.aliyuncs.com"));
        assert!(rs.is_blocked("sub.ad-foo.aliyuncs.com")); // 子域名
        assert!(!rs.is_blocked("foo.aliyuncs.com")); // 不匹配
        assert!(!rs.is_blocked("aliyuncs.com"));
    }

    #[test]
    fn test_wildcard_allowlist() {
        let mut rs = RuleSet::new();
        rs.add_rule("||*.ads.com^");
        rs.add_rule("@@||safe-*.ads.com^");
        rs.build();
        assert!(rs.is_blocked("bad-ads.ads.com"));
        assert!(!rs.is_blocked("safe-cdn.ads.com")); // 白名单优先
    }

    #[test]
    fn test_wildcard_blocked_count() {
        let mut rs = RuleSet::new();
        rs.add_rule("||ad-*.aliyuncs.com^");
        rs.add_rule("||tracker-*.example.com^");
        rs.add_rule("||plain.com^"); // 非通配符
        assert_eq!(rs.wildcard_blocked_count(), 2);
        assert_eq!(rs.blocked_count(), 3); // 2 wildcard + 1 plain
    }

    #[test]
    fn test_wildcard_case_insensitive() {
        let mut rs = RuleSet::new();
        rs.add_rule("||AD-*.example.com^");
        rs.build();
        assert!(rs.is_blocked("ad-foo.example.com"));
        assert!(rs.is_blocked("AD-FOO.example.com"));
    }

    #[test]
    fn test_wildcard_must_match_at_least_one_char() {
        let mut rs = RuleSet::new();
        rs.add_rule("||ad-*.example.com^");
        rs.build();
        // `*` 必须匹配至少一个字符（[^.]+ 不匹配空串）
        // "ad-.example.com" 中 * 部分为空，不应匹配
        assert!(!rs.is_blocked("ad-.example.com"));
        assert!(rs.is_blocked("ad-x.example.com"));
    }
}
