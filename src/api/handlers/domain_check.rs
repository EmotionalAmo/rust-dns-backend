use crate::api::middleware::auth::AuthUser;
use crate::api::AppState;
use crate::error::AppResult;
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Deserialize)]
pub struct DomainCheckRequest {
    pub domains: Vec<String>,
}

#[derive(Serialize)]
pub struct DomainCheckResult {
    pub domain: String,
    pub blocked: bool,
    pub rewrite_target: Option<String>,
    pub action: String,
}

pub async fn check_domains(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(body): Json<DomainCheckRequest>,
) -> AppResult<Json<Value>> {
    if body.domains.is_empty() {
        return Ok(Json(json!({"results": []})));
    }
    if body.domains.len() > 100 {
        return Ok(Json(json!({"error": "maximum 100 domains per request"})));
    }

    let mut results = Vec::new();
    for domain in &body.domains {
        let domain = domain.trim().to_lowercase();
        if domain.is_empty() {
            continue;
        }
        // is_blocked / check_rewrite 已改为同步方法（arc-swap 无锁读）
        let blocked = state.filter.is_blocked(&domain);
        let rewrite_target = state.filter.check_rewrite(&domain);

        let action = if let Some(ref target) = rewrite_target {
            format!("rewritten:{}", target)
        } else if blocked {
            "blocked".to_string()
        } else {
            "allowed".to_string()
        };

        results.push(DomainCheckResult {
            domain,
            blocked,
            rewrite_target,
            action,
        });
    }

    Ok(Json(json!({"results": results})))
}
