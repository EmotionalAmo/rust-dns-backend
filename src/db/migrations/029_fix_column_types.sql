-- 029_fix_column_types.sql
-- Fix column type mismatches between DB schema and application code.
-- All changes are idempotent — skips columns already at the target type.

DO $$
BEGIN
    -- query_log.time: TEXT -> TIMESTAMPTZ
    -- Root cause: 001_initial.sql defined as TEXT; app code uses timestamp operators.
    IF (SELECT data_type FROM information_schema.columns
        WHERE table_name='query_log' AND column_name='time') = 'text' THEN
        ALTER TABLE query_log ALTER COLUMN time TYPE timestamptz USING time::timestamptz;
    END IF;

    -- query_log.elapsed_ns: INTEGER -> BIGINT
    -- Root cause: originally elapsed_ms INTEGER in 001, renamed in 012 but type not changed.
    IF (SELECT data_type FROM information_schema.columns
        WHERE table_name='query_log' AND column_name='elapsed_ns') = 'integer' THEN
        ALTER TABLE query_log ALTER COLUMN elapsed_ns TYPE bigint;
    END IF;

    -- query_log.upstream_ns: INTEGER -> BIGINT
    IF (SELECT data_type FROM information_schema.columns
        WHERE table_name='query_log' AND column_name='upstream_ns') = 'integer' THEN
        ALTER TABLE query_log ALTER COLUMN upstream_ns TYPE bigint;
    END IF;

    -- query_log_templates.is_public: INTEGER -> BOOLEAN
    -- Root cause: 005_query_log_templates.sql used INTEGER; code uses boolean comparison.
    IF (SELECT data_type FROM information_schema.columns
        WHERE table_name='query_log_templates' AND column_name='is_public') = 'integer' THEN
        ALTER TABLE query_log_templates ALTER COLUMN is_public DROP DEFAULT;
        ALTER TABLE query_log_templates ALTER COLUMN is_public TYPE boolean USING (is_public != 0);
        ALTER TABLE query_log_templates ALTER COLUMN is_public SET DEFAULT false;
    END IF;

    -- upstream_latency_log.checked_at: TEXT -> TIMESTAMPTZ
    -- Root cause: 008_upstream_latency_log.sql defined as TEXT; code uses TIMESTAMP operators.
    IF (SELECT data_type FROM information_schema.columns
        WHERE table_name='upstream_latency_log' AND column_name='checked_at') = 'text' THEN
        ALTER TABLE upstream_latency_log ALTER COLUMN checked_at TYPE timestamptz USING checked_at::timestamptz;
    END IF;
END $$;
