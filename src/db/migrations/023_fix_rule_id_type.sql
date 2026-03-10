-- Migration 023: Fix column types for client_group_memberships and client_group_rules
-- This migration fixes type mismatches that existed before 006 was corrected.
-- Safe to run on both old and new databases.

-- Fix client_group_memberships.group_id (INTEGER -> BIGINT to match client_groups.id)
ALTER TABLE client_group_memberships
    ALTER COLUMN group_id TYPE BIGINT;

-- Fix client_group_rules.group_id (INTEGER -> BIGINT to match client_groups.id)
ALTER TABLE client_group_rules
    ALTER COLUMN group_id TYPE BIGINT;

-- Fix client_group_rules.rule_id (INTEGER -> TEXT to match dns_rewrites.id/custom_rules.id)
-- Need to drop constraints first
ALTER TABLE client_group_rules
    DROP CONSTRAINT IF EXISTS client_group_rules_group_id_rule_id_rule_type_key;

-- Change column type
ALTER TABLE client_group_rules
    ALTER COLUMN rule_id TYPE TEXT USING rule_id::TEXT;

-- Re-create unique constraint
ALTER TABLE client_group_rules
    ADD CONSTRAINT client_group_rules_group_id_rule_id_rule_type_key
    UNIQUE (group_id, rule_id, rule_type);

-- Re-create index
DROP INDEX IF EXISTS idx_group_rules_rule;
CREATE INDEX IF NOT EXISTS idx_group_rules_rule ON client_group_rules(rule_id, rule_type);
