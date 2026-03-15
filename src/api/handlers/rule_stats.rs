use crate::api::middleware::auth::AuthUser;
use crate::api::AppState;
use crate::error::AppResult;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Deserialize)]
pub struct StatsParams {
    hours: Option<i64>,
    limit: Option<i64>,
}

#[derive(Deserialize)]
pub struct HitDetailParams {
    hours: Option<i64>,
    limit: Option<i64>,
}

/// 规则匹配方式：普通域名 或 通配符正则 或 跳过（放行/注释/无法统计）
enum RuleMatcher {
    Domain(String),
    Wildcard(Regex),
    Skip,
}

/// 解析规则字符串，返回对应的匹配器。
/// - 放行规则 (@@...) / 注释 → Skip
/// - ||ad-*.example.com^ 等含通配符 → Wildcard(Regex)
/// - ||example.com^ / *.example.com / plain domain 等 → Domain(String)
fn parse_rule_matcher(rule: &str) -> RuleMatcher {
    let r = rule.trim();

    if r.is_empty() || r.starts_with('#') || r.starts_with('!') || r.starts_with("@@") {
        return RuleMatcher::Skip;
    }

    // AdGuard 格式：||domain^ 或 ||ad-*.example.com^
    if let Some(rest) = r.strip_prefix("||") {
        let after_caret = rest.split('^').next().unwrap_or(rest);
        let domain_raw = after_caret.split('$').next().unwrap_or(after_caret);
        let domain_str = domain_raw
            .trim_end_matches('|')
            .trim_end_matches('/')
            .trim_end_matches('.')
            .to_lowercase();

        if domain_str.is_empty() {
            return RuleMatcher::Skip;
        }

        if domain_str.contains('*') {
            // 含通配符 → 编译为 Regex
            let pattern = wildcard_to_regex(&domain_str);
            return match Regex::new(&pattern) {
                Ok(re) => RuleMatcher::Wildcard(re),
                Err(_) => RuleMatcher::Skip,
            };
        }

        return RuleMatcher::Domain(domain_str);
    }

    // 子域通配：*.example.com → 等价于 example.com 的域名规则
    if let Some(rest) = r.strip_prefix("*.") {
        let d = rest.trim().to_lowercase();
        if !d.is_empty() && !d.contains('*') {
            return RuleMatcher::Domain(d);
        }
        return RuleMatcher::Skip;
    }
    if let Some(rest) = r.strip_prefix('.') {
        let d = rest.trim().to_lowercase();
        if !d.is_empty() && !d.contains('*') {
            return RuleMatcher::Domain(d);
        }
        return RuleMatcher::Skip;
    }

    // hosts 格式：0.0.0.0 example.com 等
    if r.starts_with("0.0.0.0 ")
        || r.starts_with("127.0.0.1 ")
        || r.starts_with("::1 ")
        || r.starts_with("::0 ")
    {
        let parts: Vec<&str> = r.splitn(2, ' ').collect();
        if parts.len() == 2 {
            let d = parts[1].trim().to_lowercase();
            if !d.is_empty() && !d.contains('*') {
                return RuleMatcher::Domain(d);
            }
        }
        return RuleMatcher::Skip;
    }

    // 含通配符的裸规则：暂不支持
    if r.contains('*') {
        return RuleMatcher::Skip;
    }

    // plain domain
    if !r.contains('/') && !r.contains(' ') {
        let d = r.to_lowercase();
        if !d.is_empty() {
            return RuleMatcher::Domain(d);
        }
    }

    RuleMatcher::Skip
}

/// 将通配符域名模式转换为正则表达式字符串。
/// `*` 匹配一个或多个非点字符（label 内通配）。
/// 例：`ad-*.aliyuncs.com` → `(?i)^ad\-[^.]+\.aliyuncs\.com$`
fn wildcard_to_regex(pattern: &str) -> String {
    let mut re = String::with_capacity(pattern.len() + 16);
    re.push_str("(?i)^");
    for ch in pattern.chars() {
        match ch {
            '*' => re.push_str("[^.]+"),
            '.' => re.push_str("\\."),
            c if c.is_alphanumeric() || c == '-' || c == '_' => re.push(c),
            c => {
                re.push('\\');
                re.push(c);
            }
        }
    }
    re.push('$');
    re
}

/// 判断一条 DNS 查询域名是否命中规则域名（精确 + 子域）
fn domain_matches(question: &str, rule_domain: &str) -> bool {
    let q = question.trim_end_matches('.').to_lowercase();
    q == rule_domain || q.ends_with(&format!(".{}", rule_domain))
}

pub async fn rule_hit_stats(
    State(state): State<Arc<AppState>>,
    Query(params): Query<StatsParams>,
    _auth: AuthUser,
) -> AppResult<Json<Value>> {
    let hours = params.hours.unwrap_or(24).clamp(1, 720);
    let limit = params.limit.unwrap_or(100).clamp(1, 1000);

    // Step 1: 从 query_log 获取过去 N 小时内所有 blocked 域名的命中次数
    let blocked_rows: Vec<(String, i64, Option<String>)> = sqlx::query_as(
        "SELECT question, COUNT(*) as cnt, MAX(time) as last_seen
         FROM query_log
         WHERE status = 'blocked'
           AND time >= NOW() - ($1 * INTERVAL '1 hour')
         GROUP BY question",
    )
    .bind(hours)
    .fetch_all(&state.db)
    .await?;

    let mut question_counts: HashMap<String, (i64, Option<String>)> =
        HashMap::with_capacity(blocked_rows.len());
    for (question, cnt, last_seen) in blocked_rows {
        question_counts.insert(question.to_lowercase(), (cnt, last_seen));
    }

    // Step 2: 加载用户规则（过滤掉订阅列表规则）
    let rule_rows: Vec<(String, String, Option<String>, bool, String)> = sqlx::query_as(
        "SELECT id, rule, comment, is_enabled, created_at
         FROM custom_rules
         WHERE created_by NOT LIKE 'filter:%'
         ORDER BY created_at DESC",
    )
    .fetch_all(&state.db)
    .await?;

    let total_rules = rule_rows.len();

    // Step 3: 内存中统计每条规则的命中次数（支持普通域名 + 通配符正则）
    let mut result: Vec<Value> = rule_rows
        .into_iter()
        .map(|(id, rule, comment, is_enabled, created_at)| {
            let mut hit_count: i64 = 0;
            let mut last_seen: Option<String> = None;

            match parse_rule_matcher(&rule) {
                RuleMatcher::Domain(rule_domain) => {
                    for (question, (cnt, q_last_seen)) in &question_counts {
                        if domain_matches(question, &rule_domain) {
                            hit_count += cnt;
                            match (&last_seen, q_last_seen) {
                                (None, Some(ls)) => last_seen = Some(ls.clone()),
                                (Some(cur), Some(ls)) if ls > cur => last_seen = Some(ls.clone()),
                                _ => {}
                            }
                        }
                    }
                }
                RuleMatcher::Wildcard(re) => {
                    for (question, (cnt, q_last_seen)) in &question_counts {
                        let q = question.trim_end_matches('.');
                        if re.is_match(q) {
                            hit_count += cnt;
                            match (&last_seen, q_last_seen) {
                                (None, Some(ls)) => last_seen = Some(ls.clone()),
                                (Some(cur), Some(ls)) if ls > cur => last_seen = Some(ls.clone()),
                                _ => {}
                            }
                        }
                    }
                }
                RuleMatcher::Skip => {}
            }

            json!({
                "id": id,
                "rule": rule,
                "comment": comment,
                "is_enabled": is_enabled,
                "created_at": created_at,
                "hit_count": hit_count,
                "last_seen": last_seen,
            })
        })
        .collect();

    // Step 4: 按 hit_count DESC 排序，截取 limit
    result.sort_by(|a, b| {
        let a_hits = a["hit_count"].as_i64().unwrap_or(0);
        let b_hits = b["hit_count"].as_i64().unwrap_or(0);
        b_hits.cmp(&a_hits)
    });
    result.truncate(limit as usize);

    Ok(Json(json!({
        "data": result,
        "hours": hours,
        "total_rules": total_rules,
    })))
}

pub async fn rule_hit_detail(
    State(state): State<Arc<AppState>>,
    Path(rule_id): Path<String>,
    Query(params): Query<HitDetailParams>,
    _auth: AuthUser,
) -> AppResult<Json<Value>> {
    let hours = params.hours.unwrap_or(24).clamp(1, 720);
    let limit = params.limit.unwrap_or(20).clamp(1, 200);

    // Step 1: 查出该规则（只查用户规则，不查订阅列表规则）
    let rule_row: Option<(String, String)> = sqlx::query_as(
        "SELECT id, rule FROM custom_rules WHERE id = $1 AND created_by NOT LIKE 'filter:%'",
    )
    .bind(&rule_id)
    .fetch_optional(&state.db)
    .await?;

    let (id, rule) = match rule_row {
        Some(r) => r,
        None => return Err(crate::error::AppError::NotFound(rule_id)),
    };

    // Step 2: 解析规则匹配器
    let matcher = parse_rule_matcher(&rule);

    // Step 3: 查询过去 N 小时内所有被 blocked 的域名及计数
    let blocked_rows: Vec<(String, i64, Option<String>)> = sqlx::query_as(
        "SELECT question, COUNT(*) as cnt, MAX(time) as last_seen
         FROM query_log
         WHERE status = 'blocked'
           AND time >= NOW() - ($1 * INTERVAL '1 hour')
         GROUP BY question",
    )
    .bind(hours)
    .fetch_all(&state.db)
    .await?;

    // Step 4: 内存中过滤匹配该规则的域名
    let mut hits: Vec<Value> = Vec::new();
    for (question, cnt, last_seen) in blocked_rows {
        let matched = match &matcher {
            RuleMatcher::Domain(rule_domain) => domain_matches(&question, rule_domain),
            RuleMatcher::Wildcard(re) => {
                let q = question.trim_end_matches('.');
                re.is_match(q)
            }
            RuleMatcher::Skip => false,
        };

        if matched {
            hits.push(json!({
                "domain": question,
                "count": cnt,
                "last_seen": last_seen,
            }));
        }
    }

    // Step 5: 按 count DESC 排序，截取 limit
    hits.sort_by(|a, b| {
        let a_cnt = a["count"].as_i64().unwrap_or(0);
        let b_cnt = b["count"].as_i64().unwrap_or(0);
        b_cnt.cmp(&a_cnt)
    });
    hits.truncate(limit as usize);

    Ok(Json(json!({
        "rule_id": id,
        "rule": rule,
        "hits": hits,
        "hours": hours,
    })))
}
