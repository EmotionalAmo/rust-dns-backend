#![allow(dead_code)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct DnsRewrite {
    pub id: String,
    pub domain: String,
    pub answer: String, // Target IP address
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateRewriteRequest {
    pub domain: String,
    pub answer: String, // Target IP address
}

#[derive(Debug, Deserialize)]
pub struct UpdateRewriteRequest {
    pub domain: Option<String>,
    pub answer: Option<String>,
}
