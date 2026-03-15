-- M-026: Alert mutes — persist per-type mute state server-side
-- Replaces localStorage-based muting in the frontend.
-- alert_type is the primary key; one row per muted type.
-- muted_until NULL = muted indefinitely; TEXT ISO-8601 = muted until that timestamp.
CREATE TABLE alert_mutes (
    alert_type TEXT PRIMARY KEY,
    muted_until TEXT,
    created_at TEXT NOT NULL
);
