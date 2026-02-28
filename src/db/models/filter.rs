#![allow(dead_code)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct FilterList {
    pub id: String,
    pub name: String,
    pub url: Option<String>,
    pub is_enabled: bool,
    pub rule_count: i64,
    pub last_updated: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}
