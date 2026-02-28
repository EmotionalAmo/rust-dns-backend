-- Users
CREATE TABLE IF NOT EXISTS users (
    id          TEXT PRIMARY KEY,
    username    TEXT UNIQUE NOT NULL,
    password    TEXT NOT NULL,
    role        TEXT NOT NULL CHECK (role IN ('super_admin','admin','operator','read_only')),
    is_active   INTEGER NOT NULL DEFAULT 1,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

-- Filter lists (subscriptions)
CREATE TABLE IF NOT EXISTS filter_lists (
    id           TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    url          TEXT,
    is_enabled   INTEGER NOT NULL DEFAULT 1,
    rule_count   INTEGER NOT NULL DEFAULT 0,
    last_updated TEXT,
    created_at   TEXT NOT NULL
);

-- Custom rules (AdGuard syntax)
CREATE TABLE IF NOT EXISTS custom_rules (
    id         TEXT PRIMARY KEY,
    rule       TEXT NOT NULL,
    comment    TEXT,
    is_enabled INTEGER NOT NULL DEFAULT 1,
    created_by TEXT NOT NULL,
    created_at TEXT NOT NULL
);

-- DNS rewrites
CREATE TABLE IF NOT EXISTS dns_rewrites (
    id         TEXT PRIMARY KEY,
    domain     TEXT NOT NULL,
    answer     TEXT NOT NULL,
    created_by TEXT NOT NULL,
    created_at TEXT NOT NULL
);

-- Clients
CREATE TABLE IF NOT EXISTS clients (
    id             TEXT PRIMARY KEY,
    name           TEXT NOT NULL,
    identifiers    TEXT NOT NULL,  -- JSON: ["ip", "mac"]
    upstreams      TEXT,           -- JSON: ["tls://..."]
    filter_enabled INTEGER NOT NULL DEFAULT 1,
    tags           TEXT,           -- JSON array
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);

-- Query log
CREATE TABLE IF NOT EXISTS query_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    time        TEXT NOT NULL,
    client_ip   TEXT NOT NULL,
    client_name TEXT,
    question    TEXT NOT NULL,
    qtype       TEXT NOT NULL,
    answer      TEXT,
    status      TEXT NOT NULL CHECK (status IN ('allowed','blocked','cached','error')),
    reason      TEXT,
    upstream    TEXT,
    elapsed_ms  INTEGER
);

CREATE INDEX IF NOT EXISTS idx_query_log_time ON query_log(time DESC);
CREATE INDEX IF NOT EXISTS idx_query_log_client ON query_log(client_ip, time DESC);
CREATE INDEX IF NOT EXISTS idx_query_log_question ON query_log(question);

-- Audit log (immutable)
CREATE TABLE IF NOT EXISTS audit_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    time        TEXT NOT NULL,
    user_id     TEXT NOT NULL,
    username    TEXT NOT NULL,
    action      TEXT NOT NULL,
    resource    TEXT NOT NULL,
    resource_id TEXT,
    detail      TEXT,
    ip          TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_audit_log_time ON audit_log(time DESC);
CREATE INDEX IF NOT EXISTS idx_audit_log_user ON audit_log(user_id, time DESC);

-- Settings (key-value)
CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Insert defaults
INSERT OR IGNORE INTO settings (key, value) VALUES
    ('dns_cache_ttl', '300'),
    ('query_log_retention_days', '30'),
    ('stats_retention_days', '90'),
    ('safe_search_enabled', 'false'),
    ('parental_control_enabled', 'false');
