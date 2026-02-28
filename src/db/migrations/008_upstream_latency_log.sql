-- 008_upstream_latency_log.sql
-- Track per-upstream latency history for avg latency display (30min / 60min windows)

CREATE TABLE IF NOT EXISTS upstream_latency_log (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    upstream_id TEXT NOT NULL REFERENCES dns_upstreams(id) ON DELETE CASCADE,
    latency_ms INTEGER NOT NULL,
    success    INTEGER NOT NULL DEFAULT 1,   -- 1 = success, 0 = failure
    checked_at TEXT NOT NULL                 -- RFC3339 timestamp
);

CREATE INDEX IF NOT EXISTS idx_upstream_latency_log_upstream_checked
    ON upstream_latency_log(upstream_id, checked_at DESC);
