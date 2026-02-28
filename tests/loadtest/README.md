# Ent-DNS 性能测试快速启动指南

## 前置条件

### 1. 安装测试工具

```bash
# macOS
brew install dnsperf k6

# Linux (Ubuntu/Debian)
sudo apt-get install dnsperf
# 安装 k6
sudo apt-key adv --keyserver hkp://keyserver.ubuntu.com:80 --recv-keys C5AD17C747E3415A3642D57D77C6C491D6AC1D69
echo "deb https://dl.k6.io/deb stable main" | sudo tee /etc/apt/sources.list.d/k6.list
sudo apt-get update
sudo apt-get install k6
```

### 2. 准备测试域名列表

```bash
cd /Users/emotionalamo/Developer/Ent-DNS/projects/ent-dns/tests/loadtest

# 方法 1: 使用真实域名列表（推荐）
curl -s https://raw.githubusercontent.com/curl/curl/master/docs/examples/html-list.html | \
    grep -oP 'href="https?://[^"]+' | \
    sed 's|https://||g' | \
    sed 's|/.*||g' | \
    sort -u | \
    head -1000 > domains.txt

# 方法 2: 生成测试域名（仅用于快速验证）
for i in {1..1000}; do
    echo "test$i.example.com"
done > domains.txt
```

### 3. 启动 Ent-DNS（释放模式）

```bash
cd /Users/emotionalamo/Developer/Ent-DNS/projects/ent-dns

# 编译释放版本
cargo build --release

# 启动（需要设置 JWT_SECRET）
export ENT_DNS__DNS__PORT=5353
export ENT_DNS__DATABASE__PATH=/tmp/ent-dns-loadtest.db
export ENT_DNS__AUTH__JWT_SECRET="test-secret-for-loadtest-only-32chars-min"
./target/release/ent-dns &
DNS_PID=$!

echo "Ent-DNS 启动 PID: $DNS_PID"
```

### 4. 验证服务健康

```bash
# 测试 DNS
dig @127.0.0.1 -p 5353 example.com A +short

# 测试 API
curl http://127.0.0.1:8080/health

# 测试 Metrics
curl http://127.0.0.1:8080/metrics
```

---

## 执行测试

### 场景 1: DNS QPS 容量测试（快速）

```bash
cd /Users/emotionalamo/Developer/Ent-DNS/projects/ent-dns/tests/loadtest

# 快速测试（5 分钟/阶段）
./dns-qps-test.sh

# 查看结果
cat results/qps-test-*/comparison.txt
```

### 场景 2: API 并发写入测试

```bash
cd /Users/emotionalamo/Developer/Ent-DNS/projects/ent-dns/tests/loadtest

# 获取 JWT Token
TOKEN=$(curl -s -X POST http://127.0.0.1:8080/api/v1/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"admin","password":"admin"}' | jq -r '.token')

# 运行 k6 测试
export AUTH_TOKEN=$TOKEN
k6 run api-write-test.js --duration 10m --vus 50

# 查看结果
k6 run api-write-test.js --out json=results/k6-results.json
```

### 场景 3: 短期稳定性测试（1 小时）

```bash
cd /Users/emotionalamo/Developer/Ent-DNS/projects/ent-dns/tests/loadtest

# 1 小时稳定性测试（3600 秒）
export DURATION=3600
./stability-test.sh

# 查看对比报告
cat results/stability-*/comparison.txt
```

### 场景 4: 指标采集（独立运行）

```bash
cd /Users/emotionalamo/Developer/Ent-DNS/projects/ent-dns/tests/loadtest

# 启动指标采集（10 秒间隔，1 小时）
./collect-metrics.sh &

# 运行其他测试...

# 停止采集
pkill -f collect-metrics
```

---

## 结果分析

### 1. DNS 性能指标（dnsperf）

关键指标解读：

```
Queries per second: 999.87         # 实际 QPS
Average Latency (ms):              # 平均延迟
  All queries: 15.23               # 总体
  NOERROR: 12.45                   # 正常响应
  NXDOMAIN: 8.90                   # 域名不存在

Latency Distribution (ms):         # 延迟分布
  0-10:     150000 (50.0%)         # P50: ~10ms
  10-50:    120000 (40.0%)         # P95: ~50ms
  50-100:   25000 (8.3%)           # P99: ~100ms
```

**判断标准**：
- ✅ P95 < 100ms
- ✅ 错误率 < 0.1%
- ❌ P95 > 500ms（瓶颈）
- ❌ 错误率 > 5%（系统不稳定）

### 2. API 性能指标（k6）

关键指标解读：

```
http_req_duration:       # 响应时间
  avg=120ms              # 平均
  p(95)=250ms            # P95
  p(99)=500ms            # P99

errors: 1.50%            # 错误率
vus: 50                 # 并发虚拟用户
```

**判断标准**：
- ✅ P95 < 500ms
- ✅ 错误率 < 1%
- ❌ P95 > 1000ms（瓶颈）
- ❌ 错误率 > 5%（系统不稳定）

### 3. 资源消耗指标

查看监控日志：

```bash
# 查看内存趋势
cat results/stability-*/snapshot_*.txt | grep "总 RSS:"

# 查看磁盘增长
cat results/stability-*/snapshot_*.txt | grep "磁盘使用:"

# 查看 SQLite 锁状态
cat results/stability-*/snapshot_*.txt | grep "lock_status"
```

**判断标准**：
- ✅ 内存增长 < 20%（24 小时）
- ✅ 磁盘增长线性（无异常峰值）
- ✅ lock_status 无 "locked" 或 "pending"

---

## 瓶颈诊断

### 问题 1: P95 延迟飙升

**检查清单**：
1. CPU 使用率是否 100%？
2. SQLite 锁等待时间是否过长？
3. DNS 上游响应时间是否慢？

**诊断命令**：
```bash
# CPU 使用率
top -pid $(pgrep ent-dns)

# SQLite 锁等待
sqlite3 ent-dns.db "PRAGMA lock_status"

# 查询慢 SQL（需要开启日志）
sqlite3 ent-dns.db "PRAGMA busy_timeout"
```

### 问题 2: 错误率过高

**检查清单**：
1. 是否有 "database is locked" 错误？
2. 是否有 "timeout" 错误？
3. 是否有 panic 或崩溃？

**诊断命令**：
```bash
# 搜索数据库锁错误
grep -r "database is locked" results/

# 搜索 panic
grep -r "panic" results/
```

### 问题 3: 内存持续增长

**检查清单**：
1. 查询日志是否未清理？
2. 是否有缓存泄漏？
3. 是否有连接泄漏？

**诊断命令**：
```bash
# 查询日志数量
sqlite3 ent-dns.db "SELECT COUNT(*) FROM query_log;"

# 查看内存趋势
cat results/stability-*/snapshot_*.txt | grep "总 RSS:"
```

---

## 停止测试

```bash
# 停止 Ent-DNS
kill $DNS_PID

# 停止所有测试进程
pkill -f dnsperf
pkill -f k6
pkill -f collect-metrics
```

---

## 清理测试数据

```bash
# 删除测试数据库
rm -f /tmp/ent-dns-loadtest.db*
rm -f ent-dns.db*

# 删除结果目录（可选）
# rm -rf results/
```

---

## 故障排查

### 问题: dnsperf 找不到域名文件

**错误**: `Error opening file: domains.txt`

**解决**:
```bash
cd /Users/emotionalamo/Developer/Ent-DNS/projects/ent-dns/tests/loadtest
ls -la domains.txt
```

### 问题: k6 认证失败

**错误**: `认证失败：请检查 AUTH_TOKEN`

**解决**:
```bash
# 获取 token
TOKEN=$(curl -s -X POST http://127.0.0.1:8080/api/v1/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"admin","password":"admin"}' | jq -r '.token')

# 设置环境变量
export AUTH_TOKEN=$TOKEN

# 验证 token
curl http://127.0.0.1:8080/api/v1/users -H "Authorization: Bearer $TOKEN"
```

### 问题: SQLite WAL 文件过大

**错误**: WAL 文件超过 100MB

**解决**:
```bash
# 手动 checkpoint
sqlite3 ent-dns.db "PRAGMA wal_checkpoint(TRUNCATE);"

# 清理旧日志（需要在代码中实现自动轮转）
sqlite3 ent-dns.db "DELETE FROM query_log WHERE time < datetime('now', '-7 days');"
```

---

## 下一步

1. **基线建立**: 执行场景 1，记录当前性能基线
2. **瓶颈验证**: 执行场景 2，验证 SQLite 写入瓶颈
3. **优化实施**: 根据测试结果实施 SQLite 优化（参考 performance-load-test-plan.md）
4. **最终验证**: 重新执行测试，对比优化前后效果

---

**文档版本**: 1.0
**更新日期**: 2026-02-20
