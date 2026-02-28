#![allow(dead_code)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct QueryLogEntry {
    pub id: i64,
    pub time: DateTime<Utc>,
    pub client_ip: String,
    pub client_name: Option<String>,
    pub question: String,
    pub qtype: String,
    pub answer: Option<String>,
    pub status: String,
    pub reason: Option<String>,
    pub upstream: Option<String>,
    pub elapsed_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AuditLogEntry {
    pub id: i64,
    pub time: DateTime<Utc>,
    pub user_id: String,
    pub username: String,
    pub action: String,
    pub resource: String,
    pub resource_id: Option<String>,
    pub detail: Option<String>,
    pub ip: String,
}
