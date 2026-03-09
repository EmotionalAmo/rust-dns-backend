-- Migration 023: Fix client_group_rules.rule_id type from INTEGER to TEXT
-- The rule_id references custom_rules.id or dns_rewrites.id, both of which are TEXT (UUID).
-- This was a type mismatch that caused errors in PostgreSQL.

ALTER TABLE client_group_rules DROP CONSTRAINT IF EXISTS client_group_rules_group_id_rule_id_rule_type_key;
ALTER TABLE client_group_rules DROP COLUMN rule_id;
ALTER TABLE client_group_rules ADD COLUMN rule_id TEXT NOT NULL DEFAULT '';

-- Re-create unique constraint
ALTER TABLE client_group_rules ADD CONSTRAINT client_group_rules_group_id_rule_id_rule_type_key UNIQUE (group_id, rule_id, rule_type);

-- Re-create index
DROP INDEX IF EXISTS idx_group_rules_rule;
CREATE INDEX IF NOT EXISTS idx_group_rules_rule ON client_group_rules(rule_id, rule_type);
