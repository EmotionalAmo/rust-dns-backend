-- 028_fix_boolean_columns.sql
-- Convert remaining INTEGER boolean columns to BOOLEAN type.
-- These were originally defined as INTEGER in migration 001 but the application
-- code treats them as booleans. This brings fresh installs in line with
-- production databases that were manually ALTERed in Loop 193.
--
-- PostgreSQL cannot auto-cast DEFAULT 1 to boolean, so we must:
-- 1. Drop the default
-- 2. Alter the type with explicit USING
-- 3. Restore the boolean default

-- users.is_active
ALTER TABLE users ALTER COLUMN is_active DROP DEFAULT;
ALTER TABLE users ALTER COLUMN is_active TYPE BOOLEAN USING (is_active <> 0);
ALTER TABLE users ALTER COLUMN is_active SET DEFAULT true;

-- filter_lists.is_enabled
ALTER TABLE filter_lists ALTER COLUMN is_enabled DROP DEFAULT;
ALTER TABLE filter_lists ALTER COLUMN is_enabled TYPE BOOLEAN USING (is_enabled <> 0);
ALTER TABLE filter_lists ALTER COLUMN is_enabled SET DEFAULT true;

-- custom_rules.is_enabled
ALTER TABLE custom_rules ALTER COLUMN is_enabled DROP DEFAULT;
ALTER TABLE custom_rules ALTER COLUMN is_enabled TYPE BOOLEAN USING (is_enabled <> 0);
ALTER TABLE custom_rules ALTER COLUMN is_enabled SET DEFAULT true;

-- clients.filter_enabled
ALTER TABLE clients ALTER COLUMN filter_enabled DROP DEFAULT;
ALTER TABLE clients ALTER COLUMN filter_enabled TYPE BOOLEAN USING (filter_enabled <> 0);
ALTER TABLE clients ALTER COLUMN filter_enabled SET DEFAULT true;
