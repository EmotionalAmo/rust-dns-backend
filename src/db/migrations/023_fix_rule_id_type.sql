-- Migration 023: Fix client_group_rules.rule_id type and foreign key column types
-- This migration is idempotent and safe to run multiple times.
-- The rule_id references custom_rules.id or dns_rewrites.id, both of which are TEXT (UUID).
-- The group_id references client_groups.id, which is BIGSERIAL (BIGINT).

DO $$
BEGIN
    -- Fix client_group_memberships.group_id if it's INTEGER
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'client_group_memberships'
        AND column_name = 'group_id'
        AND data_type = 'integer'
    ) THEN
        ALTER TABLE client_group_memberships ALTER COLUMN group_id TYPE BIGINT;
    END IF;

    -- Fix client_group_rules columns if needed
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'client_group_rules'
        AND column_name = 'group_id'
        AND data_type = 'integer'
    ) THEN
        ALTER TABLE client_group_rules ALTER COLUMN group_id TYPE BIGINT;
    END IF;

    -- Fix client_group_rules.rule_id if it's INTEGER
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'client_group_rules'
        AND column_name = 'rule_id'
        AND data_type = 'integer'
    ) THEN
        -- Drop constraints and index that depend on rule_id
        ALTER TABLE client_group_rules DROP CONSTRAINT IF EXISTS client_group_rules_group_id_rule_id_rule_type_key;
        DROP INDEX IF EXISTS idx_group_rules_rule;

        -- Change column type from INTEGER to TEXT
        ALTER TABLE client_group_rules ALTER COLUMN rule_id TYPE TEXT;

        -- Re-create unique constraint
        ALTER TABLE client_group_rules ADD CONSTRAINT client_group_rules_group_id_rule_id_rule_type_key UNIQUE (group_id, rule_id, rule_type);

        -- Re-create index
        CREATE INDEX IF NOT EXISTS idx_group_rules_rule ON client_group_rules(rule_id, rule_type);
    END IF;
END $$;
