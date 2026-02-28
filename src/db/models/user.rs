#![allow(dead_code)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: String,
    pub username: String,
    #[serde(skip_serializing)]
    pub password: String,
    pub role: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    SuperAdmin,
    Admin,
    Operator,
    ReadOnly,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::SuperAdmin => "super_admin",
            Role::Admin => "admin",
            Role::Operator => "operator",
            Role::ReadOnly => "read_only",
        }
    }

    pub fn can_write(&self) -> bool {
        matches!(self, Role::SuperAdmin | Role::Admin | Role::Operator)
    }

    pub fn can_manage_users(&self) -> bool {
        matches!(self, Role::SuperAdmin | Role::Admin)
    }
}

impl User {
    pub fn new(username: String, password_hash: String, role: Role) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            username,
            password: password_hash,
            role: role.as_str().to_string(),
            is_active: true,
            created_at: now,
            updated_at: now,
        }
    }
}
