use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

use crate::api::middleware::auth::AuthUser;
use crate::api::middleware::client_ip::ClientIp;
use crate::api::AppState;
use crate::db::models::rewrite::{CreateRewriteRequest, UpdateRewriteRequest};
use crate::error::{AppError, AppResult};

#[derive(Deserialize)]
pub struct ListParams {
    page: Option<u32>,
    per_page: Option<u32>,
    search: Option<String>,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
    _auth: AuthUser,
) -> AppResult<Json<Value>> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(50).min(200) as i64;
    let offset = (page as i64 - 1) * per_page;
    let search = params.search.as_deref().unwrap_or("").trim().to_string();
    let has_search = !search.is_empty();
    let search_pattern = format!("%{}%", search);

    let total: i64 = if has_search {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM dns_rewrites WHERE domain LIKE $1 OR answer LIKE $1",
        )
        .bind(&search_pattern)
        .fetch_one(&state.db)
        .await?
    } else {
        sqlx::query_scalar("SELECT COUNT(*) FROM dns_rewrites")
            .fetch_one(&state.db)
            .await?
    };

    let rows: Vec<(String, String, String, String, String, i64)> = if has_search {
        sqlx::query_as(
            "SELECT dr.id, dr.domain, dr.answer, dr.created_by, dr.created_at,
                    COUNT(ql.id) AS hit_count
             FROM dns_rewrites dr
             LEFT JOIN query_log ql ON LOWER(ql.question) = dr.domain AND ql.reason = 'rewrite'
             WHERE dr.domain LIKE $1 OR dr.answer LIKE $1
             GROUP BY dr.id, dr.domain, dr.answer, dr.created_by, dr.created_at
             ORDER BY dr.domain ASC LIMIT $2 OFFSET $3",
        )
        .bind(&search_pattern)
        .bind(per_page)
        .bind(offset)
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as(
            "SELECT dr.id, dr.domain, dr.answer, dr.created_by, dr.created_at,
                    COUNT(ql.id) AS hit_count
             FROM dns_rewrites dr
             LEFT JOIN query_log ql ON LOWER(ql.question) = dr.domain AND ql.reason = 'rewrite'
             GROUP BY dr.id, dr.domain, dr.answer, dr.created_by, dr.created_at
             ORDER BY dr.domain ASC LIMIT $1 OFFSET $2",
        )
        .bind(per_page)
        .bind(offset)
        .fetch_all(&state.db)
        .await?
    };

    let data: Vec<Value> = rows
        .into_iter()
        .map(|(id, domain, answer, created_by, created_at, hit_count)| {
            json!({
                "id": id,
                "domain": domain,
                "answer": answer,
                "created_by": created_by,
                "created_at": created_at,
                "hit_count": hit_count,
            })
        })
        .collect();

    Ok(Json(json!({
        "data": data,
        "total": total,
        "page": page,
        "per_page": per_page,
    })))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    ClientIp(ip): ClientIp,
    auth: AuthUser,
    Json(body): Json<CreateRewriteRequest>,
) -> AppResult<Json<Value>> {
    let domain = body.domain.trim().to_lowercase();
    let answer = body.answer.trim().to_string();

    if domain.is_empty() {
        return Err(AppError::Validation("Domain cannot be empty".to_string()));
    }
    if answer.is_empty() {
        return Err(AppError::Validation(
            "Answer (target IP) cannot be empty".to_string(),
        ));
    }

    // Validate IP address format
    if answer.parse::<std::net::IpAddr>().is_err() {
        return Err(AppError::Validation(
            "Invalid IP address format".to_string(),
        ));
    }

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    let result = sqlx::query(
        "INSERT INTO dns_rewrites (id, domain, answer, created_by, created_at)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(&id)
    .bind(&domain)
    .bind(&answer)
    .bind(&auth.0.username)
    .bind(&now)
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => {
            // Hot-reload the filter engine
            state
                .filter
                .reload()
                .await
                .map_err(|e| AppError::Internal(e.to_string()))?;

            crate::db::audit::log_action(
                state.db.clone(),
                auth.0.sub.clone(),
                auth.0.username.clone(),
                "create",
                "rewrite",
                Some(id.clone()),
                Some(format!("{}={}", domain, answer)),
                ip.clone(),
            );

            Ok(Json(json!({
                "id": id,
                "domain": domain,
                "answer": answer,
                "created_by": auth.0.username,
                "created_at": now,
            })))
        }
        Err(e) => {
            if e.to_string().contains("UNIQUE constraint")
                || e.to_string().contains("duplicate key value")
                || e.to_string().contains("unique constraint")
            {
                Err(AppError::Validation(format!(
                    "Domain '{}' already has a rewrite rule",
                    domain
                )))
            } else {
                Err(AppError::Internal(e.to_string()))
            }
        }
    }
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    ClientIp(ip): ClientIp,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<UpdateRewriteRequest>,
) -> AppResult<Json<Value>> {
    // Check if rewrite exists
    let existing: Option<(String, String, String, String, String)> = sqlx::query_as(
        "SELECT id, domain, answer, created_by, created_at
         FROM dns_rewrites WHERE id = $1",
    )
    .bind(&id)
    .fetch_optional(&state.db)
    .await?;

    let (_, old_domain, old_answer, created_by, created_at) =
        existing.ok_or_else(|| AppError::NotFound(format!("Rewrite rule {} not found", id)))?;

    let domain = body
        .domain
        .map(|d| d.trim().to_lowercase())
        .unwrap_or(old_domain);
    let answer = body.answer.unwrap_or(old_answer);

    if answer.parse::<std::net::IpAddr>().is_err() {
        return Err(AppError::Validation(
            "Invalid IP address format".to_string(),
        ));
    }

    let result = sqlx::query("UPDATE dns_rewrites SET domain = $1, answer = $2 WHERE id = $3")
        .bind(&domain)
        .bind(&answer)
        .bind(&id)
        .execute(&state.db)
        .await;

    match result {
        Ok(_) => {
            // Hot-reload the filter engine
            state
                .filter
                .reload()
                .await
                .map_err(|e| AppError::Internal(e.to_string()))?;

            crate::db::audit::log_action(
                state.db.clone(),
                auth.0.sub.clone(),
                auth.0.username.clone(),
                "update",
                "rewrite",
                Some(id.clone()),
                Some(format!("{}={}", domain, answer)),
                ip.clone(),
            );

            Ok(Json(json!({
                "id": id,
                "domain": domain,
                "answer": answer,
                "created_by": created_by,
                "created_at": created_at,
            })))
        }
        Err(e) => {
            if e.to_string().contains("UNIQUE constraint")
                || e.to_string().contains("duplicate key value")
                || e.to_string().contains("unique constraint")
            {
                Err(AppError::Validation(format!(
                    "Domain '{}' already has a rewrite rule",
                    domain
                )))
            } else {
                Err(AppError::Internal(e.to_string()))
            }
        }
    }
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    ClientIp(ip): ClientIp,
    auth: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    let result = sqlx::query("DELETE FROM dns_rewrites WHERE id = $1")
        .bind(&id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("Rewrite rule {} not found", id)));
    }

    // Hot-reload so the deleted rewrite stops taking effect immediately
    state
        .filter
        .reload()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    crate::db::audit::log_action(
        state.db.clone(),
        auth.0.sub.clone(),
        auth.0.username.clone(),
        "delete",
        "rewrite",
        Some(id.clone()),
        None,
        ip.clone(),
    );

    Ok(Json(json!({"success": true})))
}
