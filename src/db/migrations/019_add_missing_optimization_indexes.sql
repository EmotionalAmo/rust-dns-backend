-- Optimization: Index for Top 10 Blocked Domains query
-- This query scans query_log for status = 'blocked' AND time >= ?
CREATE INDEX IF NOT EXISTS idx_query_log_status_time ON query_log(status, time DESC);

-- Optimization: Index for custom_rules deletion by source
-- This is used during filter list synchronization (DELETE FROM custom_rules WHERE created_by = ?)
CREATE INDEX IF NOT EXISTS idx_custom_rules_created_by ON custom_rules(created_by);
