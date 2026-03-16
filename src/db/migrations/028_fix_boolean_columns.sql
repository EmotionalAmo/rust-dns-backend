-- 028_fix_boolean_columns.sql
-- Convert remaining INTEGER boolean columns to BOOLEAN type.
-- These were originally defined as INTEGER in migration 001 but the application
-- code treats them as booleans. This brings fresh installs in line with
-- production databases that were manually ALTERed in Loop 193.
--
-- Idempotent: skips columns that are already BOOLEAN (e.g. production DB
-- that was manually ALTERed). Only converts INTEGER columns.

DO $$
BEGIN
    -- users.is_active
    IF (SELECT data_type FROM information_schema.columns
        WHERE table_name='users' AND column_name='is_active') = 'integer' THEN
        ALTER TABLE users ALTER COLUMN is_active DROP DEFAULT;
        ALTER TABLE users ALTER COLUMN is_active TYPE BOOLEAN USING (is_active <> 0);
        ALTER TABLE users ALTER COLUMN is_active SET DEFAULT true;
    END IF;

    -- filter_lists.is_enabled
    IF (SELECT data_type FROM information_schema.columns
        WHERE table_name='filter_lists' AND column_name='is_enabled') = 'integer' THEN
        ALTER TABLE filter_lists ALTER COLUMN is_enabled DROP DEFAULT;
        ALTER TABLE filter_lists ALTER COLUMN is_enabled TYPE BOOLEAN USING (is_enabled <> 0);
        ALTER TABLE filter_lists ALTER COLUMN is_enabled SET DEFAULT true;
    END IF;

    -- custom_rules.is_enabled
    IF (SELECT data_type FROM information_schema.columns
        WHERE table_name='custom_rules' AND column_name='is_enabled') = 'integer' THEN
        ALTER TABLE custom_rules ALTER COLUMN is_enabled DROP DEFAULT;
        ALTER TABLE custom_rules ALTER COLUMN is_enabled TYPE BOOLEAN USING (is_enabled <> 0);
        ALTER TABLE custom_rules ALTER COLUMN is_enabled SET DEFAULT true;
    END IF;

    -- clients.filter_enabled
    IF (SELECT data_type FROM information_schema.columns
        WHERE table_name='clients' AND column_name='filter_enabled') = 'integer' THEN
        ALTER TABLE clients ALTER COLUMN filter_enabled DROP DEFAULT;
        ALTER TABLE clients ALTER COLUMN filter_enabled TYPE BOOLEAN USING (filter_enabled <> 0);
        ALTER TABLE clients ALTER COLUMN filter_enabled SET DEFAULT true;
    END IF;
END $$;
