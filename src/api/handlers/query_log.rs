use crate::api::middleware::auth::AuthUser;
use crate::api::middleware::rbac::AdminUser;
use crate::api::AppState;
use crate::error::{AppError, AppResult};
use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use std::sync::Arc;

#[derive(Deserialize)]
pub struct QueryLogParams {
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(default)]
    offset: i64,
    status: Option<String>,
    client: Option<String>,
    domain: Option<String>,
}

fn default_limit() -> i64 {
    100
}

// Available export fields
const EXPORT_FIELDS: &[&str] = &[
    "id",
    "time",
    "client_ip",
    "client_name",
    "question",
    "qtype",
    "answer",
    "status",
    "reason",
    "upstream",
    "elapsed_ns",
    "upstream_ns",
];

// Default export fields (all except upstream for backward compatibility)
const DEFAULT_EXPORT_FIELDS: &[&str] = &[
    "id",
    "time",
    "client_ip",
    "client_name",
    "question",
    "qtype",
    "answer",
    "status",
    "reason",
    "elapsed_ns",
    "upstream_ns",
];

pub async fn list(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(params): Query<QueryLogParams>,
) -> AppResult<Json<Value>> {
    if let Some(status) = params.status.as_deref() {
        if !matches!(status, "blocked" | "allowed") {
            return Err(AppError::Validation(
                "status must be either 'blocked' or 'allowed'".to_string(),
            ));
        }
    }

    let limit = params.limit.clamp(1, 1000);

    // Build dynamic WHERE clause with SQL-level filtering (fixes fake-pagination bug)
    let mut conditions = Vec::<String>::new();
    if params.status.is_some() {
        conditions.push("status = ?".to_string());
    }
    if params.client.is_some() {
        conditions.push("client_ip LIKE ?".to_string());
    }
    if params.domain.is_some() {
        conditions.push("question LIKE ?".to_string());
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let data_sql = format!(
        "SELECT id, time, client_ip, client_name, question, qtype, answer, status, reason, elapsed_ns, upstream_ns
         FROM query_log {where_clause} ORDER BY time DESC LIMIT ? OFFSET ?"
    );
    let count_sql = format!("SELECT COUNT(*) FROM query_log {where_clause}");

    // Build and execute queries with dynamic bindings
    let rows = {
        let mut q = sqlx::query_as::<
            _,
            (
                i64,
                String,
                String,
                Option<String>,
                String,
                String,
                Option<String>,
                String,
                Option<String>,
                Option<i64>,
                Option<i64>,
            ),
        >(&data_sql);
        if let Some(ref s) = params.status {
            q = q.bind(s);
        }
        if let Some(ref c) = params.client {
            q = q.bind(format!("%{c}%"));
        }
        if let Some(ref d) = params.domain {
            q = q.bind(format!("%{d}%"));
        }
        q.bind(limit)
            .bind(params.offset)
            .fetch_all(&state.db)
            .await?
    };

    let total: i64 = {
        let mut q = sqlx::query_scalar::<_, i64>(&count_sql);
        if let Some(ref s) = params.status {
            q = q.bind(s);
        }
        if let Some(ref c) = params.client {
            q = q.bind(format!("%{c}%"));
        }
        if let Some(ref d) = params.domain {
            q = q.bind(format!("%{d}%"));
        }
        q.fetch_one(&state.db).await?
    };

    let data: Vec<Value> = rows
        .into_iter()
        .map(
            |(
                id,
                time,
                client_ip,
                client_name,
                question,
                qtype,
                answer,
                status,
                reason,
                elapsed_ns,
                upstream_ns,
            )| {
                json!({
                    "id": id,
                    "time": time,
                    "client_ip": client_ip,
                    "client_name": client_name,
                    "question": question,
                    "qtype": qtype,
                    "answer": answer,
                    "status": status,
                    "reason": reason,
                    "elapsed_ns": elapsed_ns,
                    "upstream_ns": upstream_ns,
                })
            },
        )
        .collect();

    let returned = data.len();
    Ok(Json(json!({
        "data": data,
        "total": total,
        "returned": returned,
        "offset": params.offset,
        "limit": limit,
    })))
}

#[derive(Deserialize)]
pub struct ExportParams {
    #[serde(default = "default_export_format")]
    format: String,
    #[serde(default)]
    fields: Option<String>, // comma-separated field list
    #[serde(default = "default_export_limit")]
    limit: i64,
    // Optional filter support (JSON-encoded filters from advanced filter)
    #[serde(default)]
    filters_json: Option<String>,
}

fn default_export_format() -> String {
    "csv".to_string()
}

fn default_export_limit() -> i64 {
    10000
}

pub async fn export(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Query(params): Query<ExportParams>,
) -> impl IntoResponse {
    // Parse and validate fields
    let fields: Vec<String> = if let Some(ref fields_str) = params.fields {
        fields_str
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| EXPORT_FIELDS.contains(&s.as_str()))
            .collect()
    } else {
        DEFAULT_EXPORT_FIELDS
            .iter()
            .map(|s| s.to_string())
            .collect()
    };

    if fields.is_empty() {
        return Json(json!({
            "error": "Invalid fields specified",
            "available_fields": EXPORT_FIELDS,
        }))
        .into_response();
    }

    // Build field list for SQL query
    let field_list = fields.join(", ");

    // Build WHERE clause if filters provided (advanced export)
    let (where_clause, where_bindings) = if let Some(ref filters_json) = params.filters_json {
        // Parse filters JSON and build WHERE clause
        if let Ok(filters_value) = serde_json::from_str::<Vec<serde_json::Value>>(filters_json) {
            let mut conditions = Vec::new();
            let mut bindings = Vec::new();

            for filter_value in filters_value {
                if let (Some(field), Some(op), Some(value)) = (
                    filter_value.get("field").and_then(|v| v.as_str()),
                    filter_value.get("operator").and_then(|v| v.as_str()),
                    filter_value.get("value"),
                ) {
                    // Log and skip unsupported filters
                    if let Ok((condition, value_bindings)) =
                        build_filter_condition(field, op, value)
                    {
                        conditions.push(condition);
                        bindings.extend(value_bindings);
                    }
                }
            }

            if conditions.is_empty() {
                (String::new(), Vec::new())
            } else {
                (format!("WHERE {}", conditions.join(" AND ")), bindings)
            }
        } else {
            (String::new(), Vec::new())
        }
    } else {
        (String::new(), Vec::new())
    };

    // Build and execute query
    let sql = format!(
        "SELECT {} FROM query_log {} ORDER BY time DESC LIMIT ?",
        field_list, where_clause
    );

    // P0-3 fix：DB 错误不再被 unwrap_or_default() 静默吞掉
    // 失败时返回 HTTP 500，客户端可感知错误而非收到空数据
    let rows: Vec<sqlx::sqlite::SqliteRow> = {
        let mut q = sqlx::query(&sql);
        for binding in &where_bindings {
            match binding {
                serde_json::Value::String(s) => q = q.bind(s),
                serde_json::Value::Number(n) => q = q.bind(n.as_i64().unwrap_or(0)),
                _ => {}
            }
        }
        match q.bind(params.limit).fetch_all(&state.db).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("Query log export DB query failed: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "error": "Database error during export",
                        "detail": e.to_string()
                    })),
                )
                    .into_response();
            }
        }
    };

    // Export based on format
    match params.format.as_str() {
        "json" => {
            let data: Vec<Value> = rows
                .iter()
                .map(|row| {
                    let mut obj = serde_json::Map::new();
                    for field in &fields {
                        // Try to get value from row by index/column name
                        let val: Option<Value> =
                            if let Ok(Some(v)) = row.try_get::<Option<String>, _>(&**field) {
                                Some(json!(v))
                            } else if let Ok(Some(v)) = row.try_get::<Option<i64>, _>(&**field) {
                                Some(json!(v))
                            } else if let Ok(v) = row.try_get::<String, _>(&**field) {
                                Some(json!(v))
                            } else {
                                None
                            };

                        if let Some(v) = val {
                            obj.insert(field.clone(), v);
                        }
                    }
                    json!(obj)
                })
                .collect();

            let body = serde_json::to_string_pretty(&data).unwrap_or_default();
            (
                [
                    (header::CONTENT_TYPE, "application/json"),
                    (
                        header::CONTENT_DISPOSITION,
                        "attachment; filename=\"query-logs.json\"",
                    ),
                ],
                body,
            )
                .into_response()
        }
        _ => {
            // CSV format
            let mut csv = String::new();
            csv.push_str(&fields.join(","));
            csv.push('\n');

            for row in rows {
                let values: Vec<String> = fields
                    .iter()
                    .map(|field| {
                        if let Ok(val) = row.try_get::<Option<String>, _>(&**field) {
                            escape_csv_field(&val.unwrap_or_default())
                        } else if let Ok(val) = row.try_get::<Option<i64>, _>(&**field) {
                            val.map(|v| v.to_string()).unwrap_or_default()
                        } else if let Ok(val) = row.try_get::<String, _>(&**field) {
                            escape_csv_field(&val)
                        } else {
                            String::new()
                        }
                    })
                    .collect();

                csv.push_str(&values.join(","));
                csv.push('\n');
            }

            (
                [
                    (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
                    (
                        header::CONTENT_DISPOSITION,
                        "attachment; filename=\"query-logs.csv\"",
                    ),
                ],
                csv,
            )
                .into_response()
        }
    }
}

// Build filter condition for export (simplified version)
fn build_filter_condition(
    field: &str,
    op: &str,
    value: &serde_json::Value,
) -> Result<(String, Vec<serde_json::Value>), String> {
    match (field, op) {
        ("status", "eq") => Ok((format!("{} = ?", field), vec![value.clone()])),
        ("qtype", "eq") => Ok((format!("{} = ?", field), vec![value.clone()])),
        ("question", "like") => {
            let pattern = format!("%{}%", value.as_str().unwrap_or(""));
            Ok((
                format!("{} LIKE ?", field),
                vec![serde_json::Value::String(pattern)],
            ))
        }
        ("client_ip", "like") => {
            let pattern = format!("%{}%", value.as_str().unwrap_or(""));
            Ok((
                format!("{} LIKE ?", field),
                vec![serde_json::Value::String(pattern)],
            ))
        }
        ("elapsed_ns", "gt") | ("elapsed_ns", "lt") | ("elapsed_ns", "eq") => {
            let sql_op = if op == "gt" {
                ">"
            } else if op == "lt" {
                "<"
            } else {
                "="
            };
            Ok((format!("{} {} ?", field, sql_op), vec![value.clone()]))
        }
        _ => Err(format!("Unsupported filter: {} {}", field, op)),
    }
}

// Escape CSV field (handle quotes and commas)
fn escape_csv_field(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace("\"", "\"\""))
    } else {
        value.to_string()
    }
}
