ALTER TABLE query_log RENAME COLUMN elapsed_ms TO elapsed_ns;
ALTER TABLE query_log RENAME COLUMN upstream_ms TO upstream_ns;
-- Convert historical millisecond values to nanoseconds
UPDATE query_log SET elapsed_ns = elapsed_ns * 1000000 WHERE elapsed_ns IS NOT NULL;
UPDATE query_log SET upstream_ns = upstream_ns * 1000000 WHERE upstream_ns IS NOT NULL;
