-- Migration: Add client groups support
-- Version: 006
-- Date: 2026-02-20
-- Author: interaction-cooper (Alan Cooper)
-- Description: Create tables for client grouping and rule binding

-- 1. Create client_groups table
CREATE TABLE IF NOT EXISTS client_groups (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    color TEXT NOT NULL DEFAULT '#6366f1',
    description TEXT,
    priority INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Indexes for client_groups
CREATE INDEX IF NOT EXISTS idx_client_groups_priority
ON client_groups(priority);

CREATE INDEX IF NOT EXISTS idx_client_groups_name
ON client_groups(name);

-- 2. Create client_group_memberships table
CREATE TABLE IF NOT EXISTS client_group_memberships (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    client_id TEXT NOT NULL,
    group_id INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(client_id, group_id) ON CONFLICT REPLACE
);

-- Indexes for client_group_memberships
CREATE INDEX IF NOT EXISTS idx_memberships_client
ON client_group_memberships(client_id);

CREATE INDEX IF NOT EXISTS idx_memberships_group
ON client_group_memberships(group_id);

CREATE INDEX IF NOT EXISTS idx_memberships_client_group
ON client_group_memberships(client_id, group_id);

-- 3. Create client_group_rules table
CREATE TABLE IF NOT EXISTS client_group_rules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    group_id INTEGER NOT NULL,
    rule_id INTEGER NOT NULL,
    rule_type TEXT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(group_id, rule_id, rule_type) ON CONFLICT REPLACE
);

-- Indexes for client_group_rules
CREATE INDEX IF NOT EXISTS idx_group_rules_group
ON client_group_rules(group_id);

CREATE INDEX IF NOT EXISTS idx_group_rules_rule
ON client_group_rules(rule_id, rule_type);

CREATE INDEX IF NOT EXISTS idx_group_rules_priority
ON client_group_rules(group_id, priority);
