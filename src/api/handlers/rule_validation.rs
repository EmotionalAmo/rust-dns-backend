use crate::api::middleware::auth::AuthUser;
use crate::api::validators::rule::{RuleValidationRequest, RuleValidationResponse, RuleValidator};
use crate::api::AppState;
use crate::error::AppResult;
use axum::{extract::State, Json};
use std::sync::Arc;

pub async fn validate_rule(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(req): Json<RuleValidationRequest>,
) -> AppResult<Json<RuleValidationResponse>> {
    let validator = RuleValidator::new();

    // Check cache first
    let cache_key = format!("{}:{}", req.rule_type, req.rule);
    if let Some(cached) = state.rule_validation_cache.get(&cache_key).await {
        return Ok(Json(cached));
    }

    // Validate
    let response = match validator.validate_rule(&req.rule_type, &req.rule) {
        Ok(()) => RuleValidationResponse {
            valid: true,
            error: None,
        },
        Err(error) => RuleValidationResponse {
            valid: false,
            error: Some(error),
        },
    };

    // Cache the result
    state
        .rule_validation_cache
        .insert(cache_key.clone(), response.clone())
        .await;

    Ok(Json(response))
}
