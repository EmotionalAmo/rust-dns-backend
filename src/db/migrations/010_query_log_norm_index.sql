-- W-4: Expression index on normalized question (strips trailing dot)
-- Speeds up exact-match JOIN in insights queries: rtrim(question, '.') = ad.domain
CREATE INDEX IF NOT EXISTS idx_query_log_question_norm ON query_log(rtrim(question, '.'));
