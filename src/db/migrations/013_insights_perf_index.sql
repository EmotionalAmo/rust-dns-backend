-- Insights 性能优化索引
-- 覆盖 query_log 时间范围查询（time + client_ip）
CREATE INDEX IF NOT EXISTS idx_query_log_time_client ON query_log(time, client_ip);

-- 覆盖 query_log 时间范围查询（time + status）
CREATE INDEX IF NOT EXISTS idx_query_log_time_status ON query_log(time, status);
