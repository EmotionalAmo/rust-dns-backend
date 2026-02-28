#![allow(dead_code)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CustomRule {
    pub id: String,
    pub rule: String,
    pub comment: Option<String>,
    pub is_enabled: bool,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}
