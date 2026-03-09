#![allow(dead_code)]

use anyhow::Result;
use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{query_as, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Upstream {
    pub id: String,
    pub name: String,
    pub addresses: String, // JSON array as string
    pub priority: i64,
    pub is_active: bool,
    pub health_check_enabled: bool,
    pub failover_enabled: bool,
    pub health_check_interval: i64,
    pub health_check_timeout: i64,
    pub failover_threshold: i64,
    pub health_status: String,
    pub last_health_check_at: Option<NaiveDateTime>,
    pub last_failover_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl Upstream {
    pub fn addresses_vec(&self) -> Result<Vec<String>> {
        Ok(serde_json::from_str(&self.addresses)?)
    }

    pub fn from_addresses(addresses: &[String]) -> Result<String> {
        Ok(serde_json::to_string(addresses)?)
    }

    pub fn created_at_str(&self) -> String {
        self.created_at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
    }

    pub fn updated_at_str(&self) -> String {
        self.updated_at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
    }

    pub fn last_health_check_at_str(&self) -> Option<String> {
        self.last_health_check_at
            .map(|t| t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
    }

    pub fn last_failover_at_str(&self) -> Option<String> {
        self.last_failover_at
            .map(|t| t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateUpstream {
    pub name: String,
    pub addresses: Vec<String>,
    pub priority: i64,
    pub health_check_interval: i64,
    pub health_check_timeout: i64,
    pub failover_threshold: i64,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUpstream {
    pub name: Option<String>,
    pub addresses: Option<Vec<String>>,
    pub priority: Option<i64>,
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
    pub timestamp: DateTime<Utc>,
}

pub struct UpstreamRepository;

impl UpstreamRepository {
    pub async fn list(pool: &PgPool) -> Result<Vec<Upstream>> {
        let rows =
            query_as::<_, Upstream>("SELECT * FROM dns_upstreams ORDER BY priority ASC, name ASC")
                .fetch_all(pool)
                .await?;
        Ok(rows)
    }

    pub async fn get(pool: &PgPool, id: &str) -> Result<Option<Upstream>> {
        let row = query_as::<_, Upstream>("SELECT * FROM dns_upstreams WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?;
        Ok(row)
    }

    pub async fn create(pool: &PgPool, req: CreateUpstream) -> Result<Upstream> {
        let id = Uuid::new_v4().to_string();
        let addresses = serde_json::to_string(&req.addresses)?;

        query_as::<_, Upstream>(
            "INSERT INTO dns_upstreams
                (id, name, addresses, priority, is_active, health_check_enabled,
                 failover_enabled, health_check_interval, health_check_timeout,
                 failover_threshold, health_status)
             VALUES ($1, $2, $3, $4, true, true, true, $5, $6, $7, 'unknown')
             RETURNING *",
        )
        .bind(&id)
        .bind(&req.name)
        .bind(&addresses)
        .bind(req.priority)
        .bind(req.health_check_interval)
        .bind(req.health_check_timeout)
        .bind(req.failover_threshold)
        .fetch_one(pool)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create upstream: {}", e))
    }

    pub async fn update(pool: &PgPool, id: &str, req: UpdateUpstream) -> Result<Upstream> {
        let now = Utc::now().naive_utc();
        let mut param_idx = 2usize; // $1 = updated_at

        let mut set_clauses = vec!["updated_at = $1".to_string()];

        if req.name.is_some() {
            set_clauses.push(format!("name = ${}", param_idx));
            param_idx += 1;
        }
        if req.addresses.is_some() {
            set_clauses.push(format!("addresses = ${}", param_idx));
            param_idx += 1;
        }
        if req.priority.is_some() {
            set_clauses.push(format!("priority = ${}", param_idx));
            param_idx += 1;
        }
        if req.is_active.is_some() {
            set_clauses.push(format!("is_active = ${}", param_idx));
            param_idx += 1;
        }
        if req.health_check_enabled.is_some() {
            set_clauses.push(format!("health_check_enabled = ${}", param_idx));
            param_idx += 1;
        }
        if req.failover_enabled.is_some() {
            set_clauses.push(format!("failover_enabled = ${}", param_idx));
            param_idx += 1;
        }
        if req.health_check_interval.is_some() {
            set_clauses.push(format!("health_check_interval = ${}", param_idx));
            param_idx += 1;
        }
        if req.health_check_timeout.is_some() {
            set_clauses.push(format!("health_check_timeout = ${}", param_idx));
            param_idx += 1;
        }
        if req.failover_threshold.is_some() {
            set_clauses.push(format!("failover_threshold = ${}", param_idx));
            param_idx += 1;
        }

        let set_clause = set_clauses.join(", ");
        let query = format!(
            "UPDATE dns_upstreams SET {} WHERE id = ${} RETURNING *",
            set_clause, param_idx
        );

        let mut q = sqlx::query_as::<_, Upstream>(&query).bind(now);

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

    pub async fn delete(pool: &PgPool, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM dns_upstreams WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn get_active_upstreams(pool: &PgPool) -> Result<Vec<Upstream>> {
        let rows = query_as::<_, Upstream>(
            "SELECT * FROM dns_upstreams
             WHERE is_active = true
             ORDER BY priority ASC",
        )
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    pub async fn update_health_status(pool: &PgPool, id: &str, status: &str) -> Result<()> {
        let now = Utc::now().naive_utc();
        sqlx::query(
            "UPDATE dns_upstreams
             SET health_status = $1, last_health_check_at = $2, updated_at = $2
             WHERE id = $3",
        )
        .bind(status)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn update_failover_status(pool: &PgPool, id: &str, status: &str) -> Result<()> {
        let now = Utc::now().naive_utc();
        sqlx::query(
            "UPDATE dns_upstreams
             SET health_status = $1, last_failover_at = $2, updated_at = $2
             WHERE id = $3",
        )
        .bind(status)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn log_failover(
        pool: &PgPool,
        upstream_id: &str,
        action: &str,
        reason: Option<&str>,
    ) -> Result<()> {
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO upstream_failover_log (id, upstream_id, action, reason, timestamp)
             VALUES ($1, $2, $3, $4, NOW())",
        )
        .bind(&id)
        .bind(upstream_id)
        .bind(action)
        .bind(reason)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_failover_log(pool: &PgPool, limit: i64) -> Result<Vec<FailoverLog>> {
        let rows = query_as::<_, FailoverLog>(
            "SELECT * FROM upstream_failover_log
             ORDER BY timestamp DESC
             LIMIT $1",
        )
        .bind(limit)
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }
}
