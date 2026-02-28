-- Insert Default Query Log Templates
-- File: src/db/migrations/007_default_query_log_templates.sql
-- Author: ui-duarte (Matías Duarte)
-- Date: 2026-02-20

INSERT OR IGNORE INTO query_log_templates (id, name, filters, logic, created_by, created_at, is_public)
VALUES
  -- 1. 最近拦截
  (
    '550e8400-e29b-41d4-a716-446655440001',
    '最近拦截',
    '[{"field":"status","operator":"eq","value":"blocked"},{"field":"time","operator":"relative","value":"-24h"}]',
    'AND',
    'system',
    datetime('now'),
    1
  ),

  -- 2. 慢查询
  (
    '550e8400-e29b-41d4-a716-446655440002',
    '慢查询',
    '[{"field":"elapsed_ms","operator":"gt","value":100},{"field":"time","operator":"relative","value":"-24h"}]',
    'AND',
    'system',
    datetime('now'),
    1
  ),

  -- 3. 错误查询
  (
    '550e8400-e29b-41d4-a716-446655440003',
    '错误查询',
    '[{"field":"status","operator":"eq","value":"error"},{"field":"time","operator":"relative","value":"-24h"}]',
    'AND',
    'system',
    datetime('now'),
    1
  ),

  -- 4. A 记录查询
  (
    '550e8400-e29b-41d4-a716-446655440004',
    'A 记录查询',
    '[{"field":"qtype","operator":"eq","value":"A"},{"field":"time","operator":"relative","value":"-1h"}]',
    'AND',
    'system',
    datetime('now'),
    1
  ),

  -- 5. 广告域名
  (
    '550e8400-e29b-41d4-a716-446655440005',
    '广告域名',
    '[{"field":"question","operator":"like","value":"ads"},{"field":"status","operator":"eq","value":"blocked"}]',
    'AND',
    'system',
    datetime('now'),
    1
  ),

  -- 6. IoT 设备
  (
    '550e8400-e29b-41d4-a716-446655440006',
    'IoT 设备',
    '[{"field":"client_ip","operator":"like","value":"192.168.1."},{"field":"elapsed_ms","operator":"lt","value":50}]',
    'AND',
    'system',
    datetime('now'),
    1
  );
