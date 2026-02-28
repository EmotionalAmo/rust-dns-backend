-- Query Log Templates - Saved Filter Presets
-- Migration 005
-- Author: ui-duarte
-- Date: 2026-02-20

CREATE TABLE IF NOT EXISTS query_log_templates (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    filters     TEXT NOT NULL, -- JSON: [{"field":"time","operator":"relative","value":"-24h"},...]
    logic       TEXT NOT NULL DEFAULT 'AND', -- "AND" or "OR"
    created_by  TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    is_public   INTEGER NOT NULL DEFAULT 0 -- 0 = private, 1 = visible to all users
);

CREATE INDEX IF NOT EXISTS idx_query_log_templates_created_by
    ON query_log_templates(created_by);

CREATE INDEX IF NOT EXISTS idx_query_log_templates_is_public
    ON query_log_templates(is_public);

-- 默认模板：常见查询场景
INSERT OR IGNORE INTO query_log_templates (id, name, filters, logic, created_by, created_at, is_public) VALUES
    -- 最近拦截的广告查询
    ('tpl-blocked-ads-recent', '最近拦截的广告', '[{"field":"status","operator":"eq","value":"blocked"},{"field":"time","operator":"relative","value":"-24h"}]', 'AND', 'system', '2026-02-20T00:00:00Z', 1),

    -- 慢查询（响应时间 > 100ms）
    ('tpl-slow-queries', '慢查询 (>100ms)', '[{"field":"elapsed_ms","operator":"gt","value":100},{"field":"time","operator":"relative","value":"-24h"}]', 'AND', 'system', '2026-02-20T00:00:00Z', 1),

    -- 错误查询（SERVFAIL、NXDOMAIN 等）
    ('tpl-error-queries', '错误查询', '[{"field":"status","operator":"eq","value":"error"},{"field":"time","operator":"relative","value":"-24h"}]', 'AND', 'system', '2026-02-20T00:00:00Z', 1),

    -- A 记录查询（最常见）
    ('tpl-a-queries', 'A 记录查询', '[{"field":"qtype","operator":"eq","value":"A"},{"field":"time","operator":"relative","value":"-1h"}]', 'AND', 'system', '2026-02-20T00:00:00Z', 1),

    -- 特定客户端的所有查询
    ('tpl-client-specific', '特定客户端', '[{"field":"client_ip","operator":"eq","value":"192.168.1.100"},{"field":"time","operator":"relative","value":"-7d"}]', 'AND', 'system', '2026-02-20T00:00:00Z', 0),

    -- 缓存命中率分析
    ('tpl-cache-analysis', '缓存分析', '[{"field":"time","operator":"relative","value":"-24h"}]', 'AND', 'system', '2026-02-20T00:00:00Z', 1);
