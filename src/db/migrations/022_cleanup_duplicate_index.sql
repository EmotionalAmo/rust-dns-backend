-- Cleanup: Remove duplicate index idx_query_log_client_time
-- This index (client_ip, time DESC) is identical to idx_query_log_client created in migration 001.
-- Removing it reduces write overhead without losing any query coverage.
DROP INDEX IF EXISTS idx_query_log_client_time;
