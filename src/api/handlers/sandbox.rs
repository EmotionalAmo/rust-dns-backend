use crate::api::middleware::auth::AuthUser;
use crate::dns::rules::{MatchResult, RuleSet};
use crate::error::{AppError, AppResult};
use axum::Json;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct SandboxRequest {
    pub rule: String,
    pub test_domains: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SandboxResult {
    pub domain: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct SandboxResponse {
    pub rule_valid: bool,
    pub rule_type: Option<String>,
    pub parsed_blocks: usize,
    pub parsed_allows: usize,
    pub results: Vec<SandboxResult>,
}

/// Test a custom rule against a list of domains
pub async fn test_rule(
    _auth: AuthUser,
    Json(body): Json<SandboxRequest>,
) -> AppResult<Json<SandboxResponse>> {
    if body.rule.trim().is_empty() {
        return Err(AppError::Validation("Rule cannot be empty".into()));
    }

    if body.test_domains.is_empty() {
        return Err(AppError::Validation(
            "At least one test domain is required".into(),
        ));
    }

    let mut ruleset = RuleSet::new();
    let num_rules_parsed = ruleset.add_rules_from_str(&body.rule);

    let rule_valid = num_rules_parsed > 0;
    let block_count = ruleset.blocked_count();
    let allow_count = ruleset.allowed_count();

    let rule_type = if block_count > 0 && allow_count == 0 {
        Some("block".to_string())
    } else if allow_count > 0 && block_count == 0 {
        Some("allow".to_string())
    } else if block_count > 0 && allow_count > 0 {
        Some("mixed".to_string())
    } else {
        None
    };

    let mut results = Vec::with_capacity(body.test_domains.len());
    for domain in &body.test_domains {
        let domain_cleaned = domain.trim().trim_end_matches('.');
        if domain_cleaned.is_empty() {
            continue;
        }

        let status = match ruleset.match_domain(domain_cleaned) {
            MatchResult::Blocked => "blocked".to_string(),
            MatchResult::Allowed => "allowed".to_string(),
            MatchResult::None => "unmatched".to_string(),
        };

        results.push(SandboxResult {
            domain: domain.clone(),
            status,
        });
    }

    Ok(Json(SandboxResponse {
        rule_valid,
        rule_type,
        parsed_blocks: block_count,
        parsed_allows: allow_count,
        results,
    }))
}
