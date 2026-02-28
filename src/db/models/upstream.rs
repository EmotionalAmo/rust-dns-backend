#![allow(dead_code)]

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{query_as, SqlitePool};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Upstream {
    pub id: String,
    pub name: String,
    pub addresses: String, // JSON array as string
    pub priority: i32,
    pub is_active: bool,
    pub health_check_enabled: bool,
    pub failover_enabled: bool,
    pub health_check_interval: i64,
    pub health_check_timeout: i64,
    pub failover_threshold: i64,
    pub health_status: String,
    pub last_health_check_at: Option<String>,
    pub last_failover_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Upstream {
    pub fn addresses_vec(&self) -> Result<Vec<String>> {
        Ok(serde_json::from_str(&self.addresses)?)
    }

    pub fn from_addresses(addresses: &[String]) -> Result<String> {
        Ok(serde_json::to_string(addresses)?)
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateUpstream {
    pub name: String,
    pub addresses: Vec<String>,
    pub priority: i32,
    pub health_check_interval: i64,
    pub health_check_timeout: i64,
    pub failover_threshold: i64,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUpstream {
    pub name: Option<String>,
    pub addresses: Option<Vec<String>>,
    pub priority: Option<i32>,
    pub is_active: Option<bool>,
    pub health_check_enabled: Option<bool>,
    pub failover_enabled: Option<bool>,
    pub health_check_interval: Option<i64>,
    pub health_check_timeout: Option<i64>,
    pub failover_threshold: Option<i64>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct FailoverLog {
    pub id: String,
    pub upstream_id: String,
    pub action: String,
    pub reason: Option<String>,
    pub timestamp: String,
}

pub struct UpstreamRepository;

impl UpstreamRepository {
    pub async fn list(pool: &SqlitePool) -> Result<Vec<Upstream>> {
        let rows =
            query_as::<_, Upstream>("SELECT * FROM dns_upstreams ORDER BY priority ASC, name ASC")
                .fetch_all(pool)
                .await?;
        Ok(rows)
    }

    pub async fn get(pool: &SqlitePool, id: &str) -> Result<Option<Upstream>> {
        let row = query_as::<_, Upstream>("SELECT * FROM dns_upstreams WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?;
        Ok(row)
    }

    pub async fn create(pool: &SqlitePool, req: CreateUpstream) -> Result<Upstream> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let addresses = serde_json::to_string(&req.addresses)?;

        query_as::<_, Upstream>(
            "INSERT INTO dns_upstreams
                (id, name, addresses, priority, is_active, health_check_enabled,
                 failover_enabled, health_check_interval, health_check_timeout,
                 failover_threshold, health_status, created_at, updated_at)
             VALUES (?, ?, ?, ?, 1, 1, 1, ?, ?, ?, 'unknown', ?, ?)
             RETURNING *",
        )
        .bind(&id)
        .bind(&req.name)
        .bind(&addresses)
        .bind(req.priority)
        .bind(req.health_check_interval)
        .bind(req.health_check_timeout)
        .bind(req.failover_threshold)
        .bind(&now)
        .bind(&now)
        .fetch_one(pool)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create upstream: {}", e))
    }

    pub async fn update(pool: &SqlitePool, id: &str, req: UpdateUpstream) -> Result<Upstream> {
        let now = Utc::now().to_rfc3339();

        // Build dynamic query
        let mut set_clauses = vec!["updated_at = ?"];
        let _bind_count = 1;

        if req.name.is_some() {
            set_clauses.push("name = ?");
        }
        if req.addresses.is_some() {
            set_clauses.push("addresses = ?");
        }
        if req.priority.is_some() {
            set_clauses.push("priority = ?");
        }
        if req.is_active.is_some() {
            set_clauses.push("is_active = ?");
        }
        if req.health_check_enabled.is_some() {
            set_clauses.push("health_check_enabled = ?");
        }
        if req.failover_enabled.is_some() {
            set_clauses.push("failover_enabled = ?");
        }
        if req.health_check_interval.is_some() {
            set_clauses.push("health_check_interval = ?");
        }
        if req.health_check_timeout.is_some() {
            set_clauses.push("health_check_timeout = ?");
        }
        if req.failover_threshold.is_some() {
            set_clauses.push("failover_threshold = ?");
        }

        let set_clause = set_clauses.join(", ");
        let query = format!(
            "UPDATE dns_upstreams SET {} WHERE id = ? RETURNING *",
            set_clause
        );

        let mut q = sqlx::query_as::<_, Upstream>(&query).bind(&now);

        if let Some(v) = req.name {
            q = q.bind(v);
        }
        if let Some(v) = req.addresses {
            q = q.bind(serde_json::to_string(&v)?);
        }
        if let Some(v) = req.priority {
            q = q.bind(v);
        }
        if let Some(v) = req.is_active {
            q = q.bind(v);
        }
        if let Some(v) = req.health_check_enabled {
            q = q.bind(v);
        }
        if let Some(v) = req.failover_enabled {
            q = q.bind(v);
        }
        if let Some(v) = req.health_check_interval {
            q = q.bind(v);
        }
        if let Some(v) = req.health_check_timeout {
            q = q.bind(v);
        }
        if let Some(v) = req.failover_threshold {
            q = q.bind(v);
        }
        q = q.bind(id);

        q.fetch_one(pool)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to update upstream: {}", e))
    }

    pub async fn delete(pool: &SqlitePool, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM dns_upstreams WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn get_active_upstreams(pool: &SqlitePool) -> Result<Vec<Upstream>> {
        let rows = query_as::<_, Upstream>(
            "SELECT * FROM dns_upstreams
             WHERE is_active = 1
             ORDER BY priority ASC",
        )
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    pub async fn update_health_status(pool: &SqlitePool, id: &str, status: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE dns_upstreams
             SET health_status = ?, last_health_check_at = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(status)
        .bind(&now)
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn update_failover_status(pool: &SqlitePool, id: &str, status: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE dns_upstreams
             SET health_status = ?, last_failover_at = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(status)
        .bind(&now)
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn log_failover(
        pool: &SqlitePool,
        upstream_id: &str,
        action: &str,
        reason: Option<&str>,
    ) -> Result<()> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO upstream_failover_log (id, upstream_id, action, reason, timestamp)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(upstream_id)
        .bind(action)
        .bind(reason)
        .bind(&now)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_failover_log(pool: &SqlitePool, limit: i64) -> Result<Vec<FailoverLog>> {
        let rows = query_as::<_, FailoverLog>(
            "SELECT * FROM upstream_failover_log
             ORDER BY timestamp DESC
             LIMIT ?",
        )
        .bind(limit)
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }
}
