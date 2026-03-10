-- Migration: Add client groups support
-- Version: 006
-- Date: 2026-02-20
-- Author: interaction-cooper (Alan Cooper)
-- Description: Create tables for client grouping and rule binding

-- 1. Create client_groups table
CREATE TABLE IF NOT EXISTS client_groups (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    color TEXT NOT NULL DEFAULT '#6366f1',
    description TEXT,
    priority INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (NOW()::TEXT),
    updated_at TEXT NOT NULL DEFAULT (NOW()::TEXT)
);

-- Indexes for client_groups
CREATE INDEX IF NOT EXISTS idx_client_groups_priority
ON client_groups(priority);

CREATE INDEX IF NOT EXISTS idx_client_groups_name
ON client_groups(name);

-- 2. Create client_group_memberships table
-- Note: group_id is BIGINT to match client_groups.id (BIGSERIAL)
CREATE TABLE IF NOT EXISTS client_group_memberships (
    id BIGSERIAL PRIMARY KEY,
    client_id TEXT NOT NULL,
    group_id BIGINT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (NOW()::TEXT),
    UNIQUE(client_id, group_id)
);

-- Indexes for client_group_memberships
CREATE INDEX IF NOT EXISTS idx_memberships_client
ON client_group_memberships(client_id);

CREATE INDEX IF NOT EXISTS idx_memberships_group
ON client_group_memberships(group_id);

CREATE INDEX IF NOT EXISTS idx_memberships_client_group
ON client_group_memberships(client_id, group_id);

-- 3. Create client_group_rules table
-- Note: rule_id is TEXT because it references custom_rules.id or dns_rewrites.id (both UUID/TEXT)
CREATE TABLE IF NOT EXISTS client_group_rules (
    id BIGSERIAL PRIMARY KEY,
    group_id BIGINT NOT NULL,
    rule_id TEXT NOT NULL,
    rule_type TEXT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (NOW()::TEXT),
    UNIQUE(group_id, rule_id, rule_type)
);

-- Indexes for client_group_rules
CREATE INDEX IF NOT EXISTS idx_group_rules_group
ON client_group_rules(group_id);

CREATE INDEX IF NOT EXISTS idx_group_rules_rule
ON client_group_rules(rule_id, rule_type);

CREATE INDEX IF NOT EXISTS idx_group_rules_priority
ON client_group_rules(group_id, priority);
