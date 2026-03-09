-- Add default upstream routing strategy to settings
INSERT INTO settings (key, value) VALUES ('upstream_strategy', 'priority')
ON CONFLICT DO NOTHING;
