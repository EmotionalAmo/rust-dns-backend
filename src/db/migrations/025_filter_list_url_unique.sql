-- M-025: Add UNIQUE constraint on filter_lists.url
-- Prevents duplicate filter list entries when using the "recommended filters" one-click add feature.
-- PostgreSQL: UNIQUE index allows multiple NULLs (NULL != NULL), so local/built-in lists without URLs are unaffected.
-- Dedup existing duplicates first (keep the one with the latest created_at).
DELETE FROM filter_lists
WHERE id NOT IN (
    SELECT DISTINCT ON (url) id
    FROM filter_lists
    WHERE url IS NOT NULL
    ORDER BY url, created_at DESC
)
AND url IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_filter_lists_url ON filter_lists(url)
    WHERE url IS NOT NULL;
