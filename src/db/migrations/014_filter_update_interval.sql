-- 过滤列表自动更新间隔（单位：小时，0 = 手动更新，NULL = 手动更新）
ALTER TABLE filter_lists ADD COLUMN update_interval_hours INTEGER NOT NULL DEFAULT 0;
