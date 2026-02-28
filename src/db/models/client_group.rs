use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Client group model
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ClientGroup {
    pub id: i64,
    pub name: String,
    pub color: String,
    pub description: Option<String>,
    pub priority: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Client group with client count
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientGroupWithStats {
    pub id: i64,
    pub name: String,
    pub color: String,
    pub description: Option<String>,
    pub priority: i32,
    pub client_count: i64,
    pub rule_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Client group membership
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ClientGroupMembership {
    pub id: i64,
    pub client_id: String,
    pub group_id: i64,
    pub created_at: DateTime<Utc>,
}

/// Client group rule binding
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ClientGroupRule {
    pub id: i64,
    pub group_id: i64,
    pub rule_id: String,   // TEXT: custom_rules.id or dns_rewrites.id
    pub rule_type: String, // "custom_rule" | "rewrite"
    pub priority: i32,
    pub created_at: DateTime<Utc>,
}

/// Client group rule with details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupRuleWithDetails {
    pub rule_id: i64,
    pub rule_type: String,
    pub name: String,
    pub pattern: Option<String>,
    pub domain: Option<String>,
    pub replacement: Option<String>,
    pub action: Option<String>, // "allow" | "block"
    pub priority: i32,
    pub created_at: DateTime<Utc>,
}

/// Create client group request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateClientGroupRequest {
    pub name: String,
    pub color: Option<String>,
    pub description: Option<String>,
    pub priority: Option<i32>,
}

/// Update client group request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateClientGroupRequest {
    pub name: Option<String>,
    pub color: Option<String>,
    pub description: Option<String>,
    pub priority: Option<i32>,
}

/// Reorder groups request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReorderGroupsRequest {
    pub group_ids: Vec<i64>,
}

/// Batch add clients to group request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchAddClientsRequest {
    pub client_ids: Vec<String>,
}

/// Batch remove clients from group request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchRemoveClientsRequest {
    pub client_ids: Vec<String>,
}

/// Batch move clients request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchMoveClientsRequest {
    pub client_ids: Vec<String>,
    pub from_group_id: Option<i64>,
    pub to_group_id: Option<i64>,
}

/// Batch bind rules request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchBindRulesRequest {
    pub rules: Vec<BindRuleRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindRuleRequest {
    pub rule_id: String,   // custom_rules.id or dns_rewrites.id (TEXT UUID)
    pub rule_type: String, // "custom_rule" | "rewrite"
    pub priority: Option<i32>,
}

/// Batch unbind rules request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchUnbindRulesRequest {
    pub rule_ids: Vec<String>, // TEXT UUIDs
    pub rule_type: String,
}

/// Reorder rules request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReorderRulesRequest {
    pub rule_ids: Vec<i64>,
}

/// Client with group info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientWithGroups {
    pub id: String,
    pub name: String,
    pub ip: String,
    pub mac: String,
    pub last_seen: DateTime<Utc>,
    pub query_count: i64,
    pub group_ids: Vec<i64>,
    pub group_names: Vec<String>,
}

/// Rule with source info (for DNS engine)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsRuleWithSource {
    pub id: i64,
    pub rule_type: String,
    pub pattern: Option<String>,
    pub domain: Option<String>,
    pub replacement: Option<String>,
    pub action: Option<String>,
    pub source: String, // "client" | "group" | "global"
    pub priority: i32,
    pub group_name: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Preview rules request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewRulesRequest {
    pub client_id: String,
    pub test_domains: Vec<String>,
}

/// Preview rules response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewRulesResponse {
    pub client_id: String,
    pub client_name: String,
    pub groups: Vec<String>,
    pub applied_rules: Vec<DnsRuleWithSource>,
    pub test_results: Vec<TestResult>,
    pub conflicts: Vec<RuleConflict>,
}

/// Test result for a domain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub domain: String,
    pub expected_action: String,
    pub applied_rule: Option<String>,
    pub rule_source: Option<String>,
}

/// Rule conflict
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleConflict {
    pub domain: String,
    pub rules: Vec<DnsRuleWithSource>,
    pub recommendation: String,
}
