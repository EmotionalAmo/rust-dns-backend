use crate::api::middleware::auth::AuthUser;
use crate::api::AppState;
use crate::error::AppResult;
use axum::{extract::State, Json};
use serde_json::{json, Value};
use std::sync::Arc;

pub async fn get_filter_stats(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
) -> AppResult<Json<Value>> {
    let (blocked_count, allowed_count, rewrite_count) = state.filter.stats().await;
    Ok(Json(json!({
        "blocked_rule_count": blocked_count,
        "allowed_rule_count": allowed_count,
        "rewrite_count": rewrite_count,
        "total_rule_count": blocked_count + allowed_count
    })))
}
