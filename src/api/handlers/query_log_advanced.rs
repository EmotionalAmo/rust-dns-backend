// Query Log Advanced Filtering Implementation
// File: src/api/handlers/query_log_advanced.rs
// Author: ui-duarte
// Date: 2026-02-20

use crate::api::middleware::auth::AuthUser;
use crate::api::AppState;
use crate::error::AppError;
use crate::error::AppResult;
use axum::{
    extract::{Query, State},
    Json,
};
use chrono::{Duration, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use std::sync::Arc;

// ============================================================================
// Data Structures
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct Filter {
    pub field: String,
    pub operator: String,
    pub value: Value,
}

#[derive(Debug, Deserialize)]
pub struct AdvancedQueryParams {
    #[serde(default)]
    pub filters: Vec<Filter>,
    #[serde(default = "default_logic")]
    pub logic: String, // "AND" | "OR"
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

/// POST body 版本的高级查询参数（与 AdvancedQueryParams 结构相同，但通过 JSON body 接收）
#[derive(Debug, Deserialize)]
pub struct AdvancedQueryBody {
    #[serde(default)]
    pub filters: Vec<Filter>,
    #[serde(default = "default_logic")]
    pub logic: String,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

#[derive(Debug, Deserialize)]
pub struct AggregateParams {
    #[serde(default)]
    pub filters: Vec<Filter>,
    #[serde(default, deserialize_with = "deserialize_group_by")]
    pub group_by: Vec<String>,
    #[serde(default = "default_metric")]
    pub metric: String, // "count" | "sum_elapsed_ms" | "avg_elapsed_ms"
    #[serde(default)]
    pub time_bucket: Option<String>, // "1m", "5m", "15m", "1h", "1d"
    #[serde(default = "default_top_limit")]
    pub limit: i64,
}

fn deserialize_group_by<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};

    struct GroupByVisitor;

    impl<'de> Visitor<'de> for GroupByVisitor {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string or a sequence of strings")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(vec![value.to_string()])
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut values = Vec::new();
            while let Some(value) = seq.next_element::<String>()? {
                values.push(value);
            }
            Ok(values)
        }
    }

    deserializer.deserialize_any(GroupByVisitor)
}

#[derive(Debug, Deserialize)]
pub struct TopParams {
    pub dimension: String, // "domain" | "client" | "qtype" | "upstream"
    #[serde(default = "default_metric")]
    pub metric: String,
    #[serde(default = "default_time_range")]
    pub time_range: String, // "-24h", "-7d", etc.
    #[serde(default)]
    pub filters: Vec<Filter>,
    #[serde(default = "default_top_limit")]
    pub limit: i64,
}

#[derive(Debug, Deserialize)]
pub struct SuggestParams {
    pub field: String,
    pub prefix: String,
    #[serde(default = "default_suggest_limit")]
    pub limit: i64,
}

#[derive(Debug, Deserialize)]
pub struct TemplateCreate {
    pub name: String,
    pub filters: Vec<Filter>,
    #[serde(default = "default_logic")]
    pub logic: String,
}

fn default_logic() -> String {
    "AND".to_string()
}
fn default_limit() -> i64 {
    100
}
fn default_metric() -> String {
    "count".to_string()
}
fn default_top_limit() -> i64 {
    10
}
fn default_time_range() -> String {
    "-24h".to_string()
}
fn default_suggest_limit() -> i64 {
    10
}

// ============================================================================
// Query Builder
// ============================================================================

pub struct QueryBuilder {
    conditions: Vec<String>,
    bindings: Vec<Value>,
}

impl Default for QueryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl QueryBuilder {
    pub fn new() -> Self {
        Self {
            conditions: Vec::new(),
            bindings: Vec::new(),
        }
    }

    pub fn add_filter(&mut self, filter: &Filter) -> AppResult<()> {
        let field = filter.field.clone();
        let operator = filter.operator.clone();
        let value = filter.value.clone();

        // Extract values early to avoid borrow issues
        let field_str = field.as_str();
        let operator_str = operator.as_str();

        let (condition, values) = match (field_str, operator_str) {
            // 时间范围
            ("time", "between") => {
                let arr = value.as_array().ok_or_else(|| {
                    AppError::Validation("time between requires array".to_string())
                })?;
                if arr.len() != 2 {
                    return Err(AppError::Validation(
                        "time between requires exactly 2 values".to_string(),
                    ));
                }
                (
                    "time BETWEEN ? AND ?".to_string(),
                    vec![arr[0].clone(), arr[1].clone()],
                )
            }
            ("time", op) if matches!(op, "gt" | "lt" | "gte" | "lte") => {
                let sql_op = match op {
                    "gt" => ">",
                    "lt" => "<",
                    "gte" => ">=",
                    "lte" => "<=",
                    _ => unreachable!(),
                };
                (format!("time {} ?", sql_op), vec![value])
            }
            // 相对时间（转换为绝对时间）
            ("time", "relative") => {
                let duration = value
                    .as_str()
                    .ok_or_else(|| AppError::Validation("relative time is string".to_string()))?;
                let (start, end) = parse_relative_time(duration)?;
                (
                    "time BETWEEN ? AND ?".to_string(),
                    vec![
                        Value::String(start.to_rfc3339()),
                        Value::String(end.to_rfc3339()),
                    ],
                )
            }
            // 字符串模糊匹配
            ("question" | "answer" | "client_name" | "upstream", "like") => {
                let pattern = format!("%{}%", value.as_str().unwrap_or(""));
                let field_owned = field.to_string();
                (
                    format!("{} LIKE ?", field_owned),
                    vec![Value::String(pattern)],
                )
            }
            // 枚举值
            ("status" | "qtype", "eq") => {
                let field_owned = field.to_string();
                (format!("{} = ?", field_owned), vec![value])
            }
            ("status" | "qtype", "in") => {
                let arr = value.as_array().ok_or_else(|| {
                    AppError::Validation("in operator requires array".to_string())
                })?;
                let placeholders = (0..arr.len()).map(|_| "?").collect::<Vec<_>>().join(",");
                let values = arr.to_vec();
                let field_owned = field.to_string();
                (format!("{} IN ({})", field_owned, placeholders), values)
            }
            // 数值比较（elapsed_ms：用户传毫秒，转换为纳秒后与 elapsed_ns 列比较）
            ("elapsed_ms", op) if matches!(op, "gt" | "lt" | "gte" | "lte" | "eq") => {
                let sql_op = match op {
                    "gt" => ">",
                    "lt" => "<",
                    "gte" => ">=",
                    "lte" => "<=",
                    "eq" => "=",
                    _ => unreachable!(),
                };
                let ms_value = value.as_i64().ok_or_else(|| {
                    AppError::Validation("elapsed_ms must be a number".to_string())
                })?;
                let ns_value = ms_value * 1_000_000;
                (format!("elapsed_ns {} ?", sql_op), vec![json!(ns_value)])
            }
            // 数值比较（elapsed_ns：直接与 elapsed_ns 列比较）
            ("elapsed_ns", op) if matches!(op, "gt" | "lt" | "gte" | "lte" | "eq") => {
                let sql_op = match op {
                    "gt" => ">",
                    "lt" => "<",
                    "gte" => ">=",
                    "lte" => "<=",
                    "eq" => "=",
                    _ => unreachable!(),
                };
                let ns_value = value.as_i64().ok_or_else(|| {
                    AppError::Validation("elapsed_ns must be a number".to_string())
                })?;
                (format!("elapsed_ns {} ?", sql_op), vec![json!(ns_value)])
            }
            // 原因字段
            ("reason", "eq" | "like") => {
                let op = if operator == "eq" { "=" } else { "LIKE" };
                let value_str = if operator == "like" {
                    format!("%{}%", value.as_str().unwrap_or(""))
                } else {
                    value.as_str().unwrap_or("").to_string()
                };
                (format!("reason {} ?", op), vec![Value::String(value_str)])
            }
            _ => {
                // 跳过不支持的字段/操作符
                return Ok(());
            }
        };

        self.conditions.push(condition);
        self.bindings.extend(values);
        Ok(())
    }

    pub fn build(self, logic: &str, limit: i64, offset: i64) -> (String, Vec<Value>) {
        let where_clause = if self.conditions.is_empty() {
            String::new()
        } else {
            let connector = if logic.to_uppercase() == "OR" {
                " OR "
            } else {
                " AND "
            };
            format!("WHERE {}", self.conditions.join(connector))
        };

        let sql = format!(
            "SELECT id, time, client_ip, client_name, question, qtype, answer, status, reason, upstream, elapsed_ns
             FROM query_log {where_clause} ORDER BY time DESC LIMIT ? OFFSET ?"
        );

        let mut bindings = self.bindings;
        bindings.push(json!(limit));
        bindings.push(json!(offset));

        (sql, bindings)
    }
}

fn parse_relative_time(
    duration: &str,
) -> AppResult<(chrono::DateTime<Utc>, chrono::DateTime<Utc>)> {
    let num: i64 = duration
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect::<String>()
        .parse()
        .map_err(|_| AppError::Validation(format!("Invalid relative time format: {}", duration)))?;

    let unit = duration
        .chars()
        .last()
        .ok_or_else(|| AppError::Validation("Missing time unit".to_string()))?;

    let now = Utc::now();
    let start = match unit {
        'h' => now - Duration::hours(num.abs()),
        'd' => now - Duration::days(num.abs()),
        'w' => now - Duration::weeks(num.abs()),
        'M' => now - Duration::days(num.abs() * 30),
        _ => {
            return Err(AppError::Validation(format!(
                "Unsupported time unit: {}",
                unit
            )))
        }
    };

    Ok((start, now))
}

// ============================================================================
// API Handlers
// ============================================================================

/// 高级查询日志列表
pub async fn list_advanced(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(params): Query<AdvancedQueryParams>,
) -> AppResult<Json<Value>> {
    let limit = params.limit.clamp(1, 1000);

    let mut builder = QueryBuilder::new();
    for filter in &params.filters {
        builder.add_filter(filter)?;
    }

    // Build a separate count query with the same WHERE conditions (no LIMIT/OFFSET)
    let (count_sql, count_bindings) = {
        let where_clause = if builder.conditions.is_empty() {
            String::new()
        } else {
            let connector = if params.logic.to_uppercase() == "OR" {
                " OR "
            } else {
                " AND "
            };
            format!("WHERE {}", builder.conditions.join(connector))
        };
        let sql = format!("SELECT COUNT(*) FROM query_log {}", where_clause);
        (sql, builder.bindings.clone())
    };

    let (sql, bindings) = builder.build(&params.logic, limit, params.offset);

    // Execute count query first to get total matching records
    let total: i64 = {
        let mut q = sqlx::query_scalar::<_, i64>(&count_sql);
        for binding in &count_bindings {
            match binding {
                Value::String(s) => q = q.bind(s),
                Value::Number(n) => q = q.bind(n.as_i64().unwrap_or(0)),
                _ => {}
            }
        }
        q.fetch_one(&state.db).await?
    };

    // Execute data query
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
                Option<String>,
                Option<i64>,
            ),
        >(&sql);
        for binding in &bindings {
            match binding {
                Value::String(s) => q = q.bind(s),
                Value::Number(n) => q = q.bind(n.as_i64().unwrap_or(0)),
                _ => {}
            }
        }
        q.fetch_all(&state.db).await?
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
                upstream,
                elapsed_ns,
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
                    "upstream": upstream,
                    "elapsed_ns": elapsed_ns,
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

/// 聚合统计
pub async fn aggregate(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(params): Query<AggregateParams>,
) -> AppResult<Json<Value>> {
    // 构建过滤条件
    let mut builder = QueryBuilder::new();
    for filter in &params.filters {
        builder.add_filter(filter)?;
    }

    // 构建 WHERE 子句（不包含 ORDER BY/LIMIT/OFFSET）
    let (where_clause, bindings) = if builder.conditions.is_empty() {
        (String::new(), Vec::new())
    } else {
        let connector = " AND ";
        let where_str = format!("WHERE {}", builder.conditions.join(connector));
        (where_str, builder.bindings)
    };

    // 如果没有 group_by，返回总体统计
    if params.group_by.is_empty() {
        let metric_sql = match params.metric.as_str() {
            "sum_elapsed_ms" => "COALESCE(SUM(elapsed_ns), 0) as metric",
            "avg_elapsed_ms" => "COALESCE(AVG(elapsed_ns), 0) as metric",
            _ => "COUNT(*) as metric",
        };

        let agg_sql = format!("SELECT {} FROM query_log {}", metric_sql, where_clause);

        let metric_value: i64 = {
            let mut q = sqlx::query_scalar::<_, i64>(&agg_sql);
            for binding in &bindings {
                match binding {
                    Value::String(s) => q = q.bind(s),
                    Value::Number(n) => q = q.bind(n.as_i64().unwrap_or(0)),
                    _ => {}
                }
            }
            q.fetch_one(&state.db).await?
        };

        return Ok(Json(json!({
            "data": [{
                "metric": metric_value,
            }],
            "group_by": [],
            "metric": params.metric,
        })));
    }

    // 验证 group_by 字段是否有效
    let valid_fields = [
        "client_ip",
        "client_name",
        "question",
        "qtype",
        "status",
        "upstream",
        "reason",
    ];
    for field in &params.group_by {
        if !valid_fields.contains(&field.as_str()) {
            return Err(AppError::Validation(format!(
                "Invalid group_by field: {}. Valid fields: {:?}",
                field, valid_fields
            )));
        }
    }

    // GROUP BY query
    let group_fields = params.group_by.join(", ");
    let metric_sql = match params.metric.as_str() {
        "sum_elapsed_ms" => "COALESCE(SUM(elapsed_ns), 0) as metric",
        "avg_elapsed_ms" => "COALESCE(AVG(elapsed_ns), 0) as metric",
        _ => "COUNT(*) as metric",
    };

    let agg_sql = format!(
        "SELECT {}, {} FROM query_log {} GROUP BY {} ORDER BY metric DESC LIMIT ?",
        group_fields, metric_sql, where_clause, group_fields
    );

    let rows: Vec<Value> = {
        let mut q = sqlx::query(&agg_sql);
        for binding in &bindings {
            match binding {
                Value::String(s) => q = q.bind(s),
                Value::Number(n) => q = q.bind(n.as_i64().unwrap_or(0)),
                _ => {}
            }
        }
        let group_fields_clone = params.group_by.clone();
        q.bind(params.limit)
            .fetch_all(&state.db)
            .await?
            .into_iter()
            .map(|row| {
                let mut obj = serde_json::Map::new();
                for field in &group_fields_clone {
                    let val: Option<String> = row.try_get(field.as_str()).unwrap_or(None);
                    obj.insert(field.clone(), val.map(Value::String).unwrap_or(Value::Null));
                }
                let metric: i64 = row.try_get("metric").unwrap_or(0);
                obj.insert("metric".to_string(), Value::Number(metric.into()));
                Value::Object(obj)
            })
            .collect()
    };

    Ok(Json(json!({
        "data": rows,
        "group_by": params.group_by,
        "metric": params.metric,
    })))
}

/// Top N 排行
pub async fn top(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(params): Query<TopParams>,
) -> AppResult<Json<Value>> {
    let (start, end) = parse_relative_time(&params.time_range)?;

    let field = match params.dimension.as_str() {
        "domain" => "question",
        "client" => "client_ip",
        "qtype" => "qtype",
        "upstream" => "upstream",
        _ => {
            return Err(AppError::Validation(format!(
                "Invalid dimension: {}",
                params.dimension
            )))
        }
    };

    let sql = format!(
        "SELECT {field} as value, COUNT(*) as count
         FROM query_log
         WHERE time BETWEEN ? AND ?
         GROUP BY {field}
         ORDER BY count DESC
         LIMIT ?",
        field = field
    );

    let rows: Vec<(String, i64)> = sqlx::query_as(&sql)
        .bind(start.to_rfc3339())
        .bind(end.to_rfc3339())
        .bind(params.limit)
        .fetch_all(&state.db)
        .await?;

    let key = format!("top_{}", params.dimension);
    let mut result = serde_json::Map::new();
    result.insert(key, serde_json::to_value(rows)?);

    Ok(Json(result.into()))
}

/// 智能提示（自动补全）
/// 支持基于历史查询热度排序（最近 30 天内查询次数最多的排在前面）
/// 结果缓存 60s，避免每次 LIKE + GROUP BY + COUNT 全表聚合
pub async fn suggest(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(params): Query<SuggestParams>,
) -> AppResult<Json<Value>> {
    let field = match params.field.as_str() {
        "question" | "client_ip" | "client_name" | "upstream" => params.field.clone(),
        _ => {
            return Err(AppError::Validation(format!(
                "Invalid field: {}",
                params.field
            )))
        }
    };

    let cache_key = format!("{}:{}:{}", field, params.prefix, params.limit);
    let db = state.db.clone();
    let prefix = params.prefix.clone();
    let prefix_resp = prefix.clone();
    let limit = params.limit;
    let field_resp = field.clone();

    let suggestions = state
        .suggest_cache
        .try_get_with(cache_key, async move {
            // 使用 30 天窗口内的历史查询热度排序
            let thirty_days_ago = Utc::now() - Duration::days(30);

            sqlx::query_scalar::<_, String>(&format!(
                "SELECT DISTINCT {}
                 FROM query_log
                 WHERE {} LIKE ? AND time >= ?
                 GROUP BY {}
                 ORDER BY COUNT(*) DESC, {} ASC
                 LIMIT ?",
                field, field, field, field
            ))
            .bind(format!("{}%", prefix))
            .bind(thirty_days_ago.to_rfc3339())
            .bind(limit)
            .fetch_all(&db)
            .await
            .map_err(AppError::Database)
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(json!({
        "suggestions": suggestions,
        "field": field_resp,
        "prefix": prefix_resp,
        "count": suggestions.len(),
    })))
}

/// 高级查询日志列表（POST 版本，filters 通过 JSON body 传递）
pub async fn list_advanced_post(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(body): Json<AdvancedQueryBody>,
) -> AppResult<Json<Value>> {
    let limit = body.limit.clamp(1, 1000);

    let mut builder = QueryBuilder::new();
    for filter in &body.filters {
        builder.add_filter(filter)?;
    }

    // Build a separate count query with the same WHERE conditions (no LIMIT/OFFSET)
    let (count_sql, count_bindings) = {
        let where_clause = if builder.conditions.is_empty() {
            String::new()
        } else {
            let connector = if body.logic.to_uppercase() == "OR" {
                " OR "
            } else {
                " AND "
            };
            format!("WHERE {}", builder.conditions.join(connector))
        };
        let sql = format!("SELECT COUNT(*) FROM query_log {}", where_clause);
        (sql, builder.bindings.clone())
    };

    let (sql, bindings) = builder.build(&body.logic, limit, body.offset);

    // Execute count query first to get total matching records
    let total: i64 = {
        let mut q = sqlx::query_scalar::<_, i64>(&count_sql);
        for binding in &count_bindings {
            match binding {
                Value::String(s) => q = q.bind(s),
                Value::Number(n) => q = q.bind(n.as_i64().unwrap_or(0)),
                _ => {}
            }
        }
        q.fetch_one(&state.db).await?
    };

    // Execute data query
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
                Option<String>,
                Option<i64>,
            ),
        >(&sql);
        for binding in &bindings {
            match binding {
                Value::String(s) => q = q.bind(s),
                Value::Number(n) => q = q.bind(n.as_i64().unwrap_or(0)),
                _ => {}
            }
        }
        q.fetch_all(&state.db).await?
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
                upstream,
                elapsed_ns,
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
                    "upstream": upstream,
                    "elapsed_ns": elapsed_ns,
                })
            },
        )
        .collect();

    let returned = data.len();

    Ok(Json(json!({
        "data": data,
        "total": total,
        "returned": returned,
        "offset": body.offset,
        "limit": limit,
    })))
}
