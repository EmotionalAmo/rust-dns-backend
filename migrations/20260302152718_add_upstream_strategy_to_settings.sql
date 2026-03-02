-- Add default upstream_strategy to settings table
INSERT OR IGNORE INTO settings (key, value) VALUES ('upstream_strategy', 'priority');
