-- Migration 015: 为 query_log 添加预计算 app_id 列
--
-- 原因：insights.rs 中 top_apps / app_trend 查询使用 LIKE '%.' || domain 前缀通配符
-- 做 JOIN，无法利用索引，在 query_log 百万级记录时会触发全表扫描。
--
-- 解决方案：在写入时同步匹配 app_id（via INSERT 子查询），查询时直接 JOIN app_id 列。

ALTER TABLE query_log ADD COLUMN app_id INTEGER REFERENCES app_catalog(id);

-- 索引：直接 JOIN 使用
CREATE INDEX IF NOT EXISTS idx_query_log_app_id ON query_log(app_id) WHERE app_id IS NOT NULL;

-- 复合索引：top_apps 的 time 过滤 + GROUP BY ac.id
CREATE INDEX IF NOT EXISTS idx_query_log_app_time ON query_log(app_id, time) WHERE app_id IS NOT NULL;

-- 回填历史数据
-- 警告：对大型数据库（>100 万条）此操作可能耗时较长，建议在低峰期执行
UPDATE query_log
SET app_id = (
    SELECT ad.app_id
    FROM app_domains ad
    WHERE rtrim(query_log.question, '.') = ad.domain
       OR rtrim(query_log.question, '.') LIKE '%.' || ad.domain
    LIMIT 1
)
WHERE app_id IS NULL;
