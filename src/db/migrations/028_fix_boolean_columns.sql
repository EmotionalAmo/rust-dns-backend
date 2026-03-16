-- 028_fix_boolean_columns.sql
-- Convert remaining INTEGER boolean columns to BOOLEAN type.
-- These were originally defined as INTEGER in migration 001 but the application
-- code treats them as booleans. This brings fresh installs in line with
-- production databases that were manually ALTERed in Loop 193.

ALTER TABLE users
    ALTER COLUMN is_active TYPE BOOLEAN USING (is_active <> 0);

ALTER TABLE filter_lists
    ALTER COLUMN is_enabled TYPE BOOLEAN USING (is_enabled <> 0);

ALTER TABLE custom_rules
    ALTER COLUMN is_enabled TYPE BOOLEAN USING (is_enabled <> 0);

ALTER TABLE clients
    ALTER COLUMN filter_enabled TYPE BOOLEAN USING (filter_enabled <> 0);
