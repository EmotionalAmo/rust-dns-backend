-- Add default upstream routing strategy to settings
INSERT OR IGNORE INTO settings (key, value) VALUES ('upstream_strategy', 'priority');
