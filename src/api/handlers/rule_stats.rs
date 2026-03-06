use crate::api::middleware::auth::AuthUser;
use crate::api::AppState;
use crate::error::AppResult;
use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Deserialize)]
pub struct StatsParams {
    hours: Option<i64>,
    limit: Option<i64>,
}

/// 从规则字符串中提取主域名（用于匹配 blocked 域名）
/// 返回 None 表示该规则不统计拦截命中（放行规则或含内嵌通配符）
fn extract_domain_from_rule(rule: &str) -> Option<String> {
    let r = rule.trim();

    // 放行规则（@@||...）不统计拦截命中
    if r.starts_with("@@") {
        return None;
    }

    // AdGuard 格式：||example.com^ 或 ||example.com^$options
    if let Some(rest) = r.strip_prefix("||") {
        // 截取到 ^ 之前
        let domain = if let Some(pos) = rest.find('^') {
            &rest[..pos]
        } else {
            rest
        };
        // 含内嵌通配符（如 ad-*.example.com）跳过
        if domain.contains('*') {
            return None;
        }
        let d = domain.trim().to_lowercase();
        if d.is_empty() {
            return None;
        }
        return Some(d);
    }

    // 子域通配：*.example.com 或 .example.com
    if let Some(rest) = r.strip_prefix("*.") {
        let d = rest.trim().to_lowercase();
        if d.is_empty() || d.contains('*') {
            return None;
        }
        return Some(d);
    }
    if let Some(rest) = r.strip_prefix('.') {
        let d = rest.trim().to_lowercase();
        if d.is_empty() || d.contains('*') {
            return None;
        }
        return Some(d);
    }

    // hosts 格式：0.0.0.0 example.com 或 127.0.0.1 example.com
    if r.starts_with("0.0.0.0 ")
        || r.starts_with("127.0.0.1 ")
        || r.starts_with("::1 ")
        || r.starts_with("::0 ")
    {
        let parts: Vec<&str> = r.splitn(2, ' ').collect();
        if parts.len() == 2 {
            let d = parts[1].trim().to_lowercase();
            if !d.is_empty() && !d.contains('*') {
                return Some(d);
            }
        }
        return None;
    }

    // 含内嵌通配符的裸规则跳过
    if r.contains('*') {
        return None;
    }

    // plain domain（不含 / 不含空格、不含 # 和 !）
    if !r.starts_with('#') && !r.starts_with('!') && !r.contains('/') && !r.contains(' ') {
        let d = r.to_lowercase();
        if !d.is_empty() {
            return Some(d);
        }
    }

    None
}

/// 判断一条 DNS 查询域名是否命中规则域名
fn domain_matches(question: &str, rule_domain: &str) -> bool {
    let q = question.trim_end_matches('.').to_lowercase();
    q == rule_domain || q.ends_with(&format!(".{}", rule_domain))
}

pub async fn rule_hit_stats(
    State(state): State<Arc<AppState>>,
    Query(params): Query<StatsParams>,
    _auth: AuthUser,
) -> AppResult<Json<Value>> {
    let hours = params.hours.unwrap_or(24).max(1).min(720);
    let limit = params.limit.unwrap_or(100).max(1).min(1000);

    // Step 1: 从 query_log 获取过去 N 小时内所有 blocked 域名的命中次数
    // 使用 datetime 函数限制扫描范围，避免全表扫描
    let blocked_rows: Vec<(String, i64, Option<String>)> = sqlx::query_as(
        "SELECT question, COUNT(*) as cnt, MAX(time) as last_seen
         FROM query_log
         WHERE status = 'blocked'
           AND time >= datetime('now', ? || ' hours')
         GROUP BY question",
    )
    .bind(format!("-{}", hours))
    .fetch_all(&state.db)
    .await?;

    // 构建 question -> (count, last_seen) 的 HashMap
    let mut question_counts: HashMap<String, (i64, Option<String>)> =
        HashMap::with_capacity(blocked_rows.len());
    for (question, cnt, last_seen) in blocked_rows {
        question_counts.insert(question.to_lowercase(), (cnt, last_seen));
    }

    // Step 2: 加载用户规则（过滤掉订阅列表规则）
    let rule_rows: Vec<(String, String, Option<String>, i64, String)> = sqlx::query_as(
        "SELECT id, rule, comment, is_enabled, created_at
         FROM custom_rules
         WHERE created_by NOT LIKE 'filter:%'
         ORDER BY created_at DESC",
    )
    .fetch_all(&state.db)
    .await?;

    let total_rules = rule_rows.len();

    // Step 3: 内存中统计每条规则的命中次数
    let mut result: Vec<Value> = rule_rows
        .into_iter()
        .map(|(id, rule, comment, is_enabled, created_at)| {
            let mut hit_count: i64 = 0;
            let mut last_seen: Option<String> = None;

            if let Some(rule_domain) = extract_domain_from_rule(&rule) {
                for (question, (cnt, q_last_seen)) in &question_counts {
                    if domain_matches(question, &rule_domain) {
                        hit_count += cnt;
                        // 取最新的 last_seen
                        match (&last_seen, q_last_seen) {
                            (None, Some(ls)) => last_seen = Some(ls.clone()),
                            (Some(cur), Some(ls)) if ls > cur => last_seen = Some(ls.clone()),
                            _ => {}
                        }
                    }
                }
            }

            json!({
                "id": id,
                "rule": rule,
                "comment": comment,
                "is_enabled": is_enabled == 1,
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
