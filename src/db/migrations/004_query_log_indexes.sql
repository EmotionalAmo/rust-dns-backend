-- Query Log Advanced Filtering - Index Optimizations
-- Migration 004
-- Author: ui-duarte
-- Date: 2026-02-20

-- 1. 复合索引：时间 + 状态（最常见查询模式）
-- 支持查询：WHERE time > ? AND status = ?
CREATE INDEX IF NOT EXISTS idx_query_log_time_status
    ON query_log(time DESC, status);

-- 2. 复合索引：时间 + 响应时间
-- 支持查询：WHERE time > ? AND elapsed_ms > ?
CREATE INDEX IF NOT EXISTS idx_query_log_time_elapsed
    ON query_log(time DESC, elapsed_ms);

-- 3. 复合索引：客户端 IP + 时间
-- 支持查询：WHERE client_ip = ? AND time > ?
CREATE INDEX IF NOT EXISTS idx_query_log_client_time
    ON query_log(client_ip, time DESC);

-- 4. 上游服务器 + 时间
-- 支持查询：WHERE upstream = ? AND time > ?
CREATE INDEX IF NOT EXISTS idx_query_log_upstream_time
    ON query_log(upstream, time DESC);

-- 5. 部分索引：仅索引 blocked 状态（减少索引大小）
-- 支持查询：WHERE status = 'blocked' AND time > ?
CREATE INDEX IF NOT EXISTS idx_query_log_blocked_time
    ON query_log(time DESC)
    WHERE status = 'blocked';

-- 6. 部分索引：仅索引 error 状态
-- 支持查询：WHERE status = 'error' AND time > ?
CREATE INDEX IF NOT EXISTS idx_query_log_error_time
    ON query_log(time DESC)
    WHERE status = 'error';

-- 7. 部分索引：仅索引 cached 状态
-- 支持查询：WHERE status = 'cached' AND time > ?
CREATE INDEX IF NOT EXISTS idx_query_log_cached_time
    ON query_log(time DESC)
    WHERE status = 'cached';

-- 8. 查询类型 + 时间
-- 支持查询：WHERE qtype = 'A' AND time > ?
CREATE INDEX IF NOT EXISTS idx_query_log_qtype_time
    ON query_log(qtype, time DESC);

-- 9. 响应码 + 时间（未来扩展：reason 字段存储 DNS RCODE）
-- 支持：WHERE reason = 'NXDOMAIN' AND time > ?
CREATE INDEX IF NOT EXISTS idx_query_log_reason_time
    ON query_log(reason, time DESC);

-- 10. 性能监控：在 settings 表中记录索引创建时间
INSERT OR IGNORE INTO settings (key, value)
VALUES ('query_log_indexes_created_at', '2026-02-20T00:00:00Z');

-- 11. 查询优化建议（注释）
-- - 对于 100 万条记录，这些索引应将查询时间从 500ms+ 降至 10-50ms
-- - 部分索引（WHERE）可减少索引大小 60-80%
-- - 定期执行 VACUUM 回收空间（建议每月一次）

-- 12. FTS5 全文索引（可选，用于域名模糊匹配）
-- CREATE VIRTUAL TABLE IF NOT EXISTS query_log_fts
-- USING fts5(question, content=query_log, content_rowid=id);
-- 使用：SELECT ql.* FROM query_log ql JOIN query_log_fts fts ON ql.id = fts.rowid WHERE question MATCH 'ads*'
