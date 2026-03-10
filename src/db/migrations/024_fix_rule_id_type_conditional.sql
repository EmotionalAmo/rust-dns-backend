-- Migration 024: Make migration 023 column-type fixes conditional (idempotent)
-- Migration 023 used unconditional ALTER TABLE statements that acquire ACCESS EXCLUSIVE
-- locks even when columns are already the correct type. On fresh databases (e.g. CI),
-- concurrent test binaries calling setup_db() in parallel would contend on these locks,
-- causing JOIN queries in client_groups_e2e tests to be blocked and fail silently.
-- This migration replaces those unconditional ALTERs with conditional DO $$ ... END$$
-- blocks that only execute when the column type actually needs to change.

-- Fix client_group_memberships.group_id (only if not already BIGINT)
DO $$
BEGIN
  IF (SELECT data_type FROM information_schema.columns
      WHERE table_name = 'client_group_memberships'
        AND column_name = 'group_id'
        AND table_schema = 'public') != 'bigint' THEN
    ALTER TABLE client_group_memberships
        ALTER COLUMN group_id TYPE BIGINT;
  END IF;
END$$;

-- Fix client_group_rules.group_id (only if not already BIGINT)
DO $$
BEGIN
  IF (SELECT data_type FROM information_schema.columns
      WHERE table_name = 'client_group_rules'
        AND column_name = 'group_id'
        AND table_schema = 'public') != 'bigint' THEN
    ALTER TABLE client_group_rules
        ALTER COLUMN group_id TYPE BIGINT;
  END IF;
END$$;

-- Fix client_group_rules.rule_id (only if not already TEXT)
-- Includes dropping and re-creating the unique constraint around the ALTER
DO $$
BEGIN
  IF (SELECT data_type FROM information_schema.columns
      WHERE table_name = 'client_group_rules'
        AND column_name = 'rule_id'
        AND table_schema = 'public') != 'text' THEN
    -- Drop unique constraint before altering type
    ALTER TABLE client_group_rules
        DROP CONSTRAINT IF EXISTS client_group_rules_group_id_rule_id_rule_type_key;

    -- Change column type
    ALTER TABLE client_group_rules
        ALTER COLUMN rule_id TYPE TEXT USING rule_id::TEXT;

    -- Re-create unique constraint
    ALTER TABLE client_group_rules
        ADD CONSTRAINT client_group_rules_group_id_rule_id_rule_type_key
        UNIQUE (group_id, rule_id, rule_type);
  END IF;
END$$;

-- Re-create index (always safe due to IF NOT EXISTS)
CREATE INDEX IF NOT EXISTS idx_group_rules_rule ON client_group_rules(rule_id, rule_type);
