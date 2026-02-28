-- DNS upstream servers
CREATE TABLE IF NOT EXISTS dns_upstreams (
    id                     TEXT PRIMARY KEY,
    name                   TEXT NOT NULL,
    addresses              TEXT NOT NULL,     -- JSON array: ["1.1.1.1:53", "8.8.8.8:53"]
    priority               INTEGER NOT NULL DEFAULT 1,   -- 1=primary, 2=secondary
    is_active              INTEGER NOT NULL DEFAULT 1,
    health_check_enabled   INTEGER NOT NULL DEFAULT 1,
    failover_enabled       INTEGER NOT NULL DEFAULT 1,
    health_check_interval  INTEGER NOT NULL DEFAULT 30,  -- seconds
    health_check_timeout   INTEGER NOT NULL DEFAULT 5,   -- seconds
    failover_threshold     INTEGER NOT NULL DEFAULT 3,  -- consecutive failures before failover
    health_status          TEXT NOT NULL DEFAULT 'unknown',
    last_health_check_at   TEXT,
    last_failover_at       TEXT,
    created_at             TEXT NOT NULL,
    updated_at             TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_dns_upstreams_priority ON dns_upstreams(priority, is_active);

-- Upstream failover log
CREATE TABLE IF NOT EXISTS upstream_failover_log (
    id          TEXT PRIMARY KEY,
    upstream_id TEXT NOT NULL,
    action      TEXT NOT NULL,  -- 'health_check_failed', 'failover_triggered', 'recovered'
    reason      TEXT,
    timestamp   TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_upstream_failover_log_time ON upstream_failover_log(timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_upstream_failover_log_upstream ON upstream_failover_log(upstream_id, timestamp DESC);

-- Seed default upstreams
INSERT OR IGNORE INTO dns_upstreams (id, name, addresses, priority, is_active, health_status, created_at, updated_at)
VALUES
    ('primary-cloudflare', 'Cloudflare Primary', '["1.1.1.1:53", "1.0.0.1:53"]', 1, 1, 'healthy', datetime('now'), datetime('now')),
    ('secondary-google', 'Google DNS', '["8.8.8.8:53", "8.8.4.4:53"]', 2, 1, 'healthy', datetime('now'), datetime('now'));
