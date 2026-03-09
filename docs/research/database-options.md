# DNS 过滤服务数据库选型调研报告

**调研人**: Ben Thompson (research-thompson)
**日期**: 2026-03-09

---

## 1. 执行摘要 (Executive Summary)

本报告评估 DNS 过滤服务 `rust-dns-backend` 在当前数据规模（107MB 活跃 + 1.3GB 历史）下的数据库选型方案。现有系统使用 SQLite 作为主数据库，采用 sqlx 进行查询，并启用了大量优化（WAL 模式、 PRAGMA 参数调优, 扶持了异步批量写入架构。

当前数据规模（14 设备, 1.4GB)尚未触达 SQLite 的理论瓶颈，但 但随着数据增长和SQLite 可能面临挑战。需要评估替代方案的可行性和。

---

## 2. 当前系统分析

### 2.1 数据模型

```
query_log (核心写入表)
├── id: INTEGER PRIMARY KEY
├── time: TEXT (ISO 8601)
├── client_ip: TEXT
├── question: TEXT
├── qtype: TEXT
├── status: TEXT (allowed/blocked/cached/error)
├── reason: TEXT
├── answer: TEXT
├── elapsed_ns: INTEGER
├── upstream_ns: INTEGER (毫秒级)
├── upstream: TEXT
├── app_id: INTEGER (外键，预计算应用分类)
```

**关键索引**:
- `idx_query_log_time` - 时间范围查询 + 排序
- `idx_query_log_status_time` - 阻断域名 Top N
- `idx_query_log_client_time` - 按客户端查询
- `idx_query_log_elapsed` - 兙慢查询排序
- `idx_query_log_upstream_time` - 上游延迟分析
- `idx_query_log_blocked_time` - 部分索引：过滤阻塞状态

- `idx_query_log_error_time` - 部分索引：过滤错误状态

**数据流**:
- DNS Handler → QueryLogWriter (批量写入)
  - insights.rs (分析查询)
  - query_log_templates.rs(模板 CRUD)
- alerts(告警)
- app_catalog(应用分类)

- clients(客户端管理)
- custom_rules(过滤规则)
- dns_rewrites(DNS 重写)
- filter_lists(过滤列表订阅)
- users(用户管理)
- settings(配置)

- audit_log(审计日志)

**写入模式**:
- 批量写入: 500 条/批次，- 单事务
- Channel 背压保护 (32K 容量)
- WAL 模式: 已启用
- PRAGMA 优化: WAL, synchronous=NORMAL, cache_size=64MB, mmap_size=256MB, wal_autocheckpoint=1000, busy_timeout=5s, temp_store=MEMORY

- 连接池: 20 个连接
- 索引策略:
  - 9 个索引优化查询性能
  - 批量写入减少写放大
  - 异步写入不阻塞 DNS hot path

### 2.2 懟量分析

**查询模式**:
- 按时间范围过滤 (`WHERE time >= ?`)
- 按客户端/设备聚合 (`GROUP by client_ip`)
- Top N 应用/域名分析 (`top_apps, app_trend, top_domains`)
- 异常检测 (`get_anomalies`) - 宣读取历史数据计算统计

- 复杂聚合查询（如 SUM, COUNT() GROUP BY）
- 排序使用 sigma 检测异常

- 娡板系统 (query_log_templates)

**写入特点**:
- **批量写入**: 500 条/批次， 2 秒间隔
- **单事务**: SQLite 事务开销约 50-100ms
- **背压保护**: 32K channel 容量，hot path 静默丢弃
- **稳定性**: 无外部依赖， 长期运行无运维负担

- **数据量**: 1.4GB (当前 107MB + 1.3GB 历史) 约 160 万条记录
- **Schema 灵活**: INTEGER PRIMARY KEY, TEXT 类型, JSON 刖 时间序列数据

- **预计算字段**: app_id 作为外键支持复杂查询

### 2.2 数据特征

| 维度 | 描述 |
|------|------|
| **写入量** | | | 高 QPS，批量写入 500 条/2s |
| **时间序列数据** | query_log 是核心写入表， 按时间范围查询是主要场景。 时间范围查询使用 `datetime('now', '-N hours')` 函数， 索引优化了性能。 但 但时间序列索引 `idx_query_log_time` 在 `time` 列上效果有限。 对于更大范围的时间窗口，性能会急剧下降。 如果使用 SQLite 的 Partial 索引（如 `strftime` 来提取时间部分，虽然 SQLite 攄持，但对于小型数据集表现良好。 但 对于时间窗口查询，全表扫描是效率低。 并发写入会成为瓶颈。因为：
SQLite 是单文件数据库，同一事务内的写入是原子操作，存在写入竞争。当多个连接同时写入时，会触发锁争争。

数据增长会导致性能下降。 但 SQLite 的水平扩展能力有限，例如分区、复制、并发控制等特性。 另外全文搜索需要依赖 `LIKE` 操作，跨服务器查询时会出现瓶颈。 在分布式场景下需要额外的服务协调和复杂性

| 数据库 | PostgreSQL | MySQL/MariaDB | TimescaleDB | ClickHouse |
| ---||---|----------|------------------|------------------|------------------|---------------|------------------|---------------|
|------------------|----------|
|--------------------|
| 优化 | 韧性 | | | | | | 篇单 | 静态部署 | ✅ 优秀 | 中等 | 较好 | 中等 | 需要额外部署和维护 | ⭐ | ⭐ | ⭐ | ⭐ | ⭐ | ✅ 优秀， 召整的生态支持 | ✅ sqlx、 diesel、 sea-orm | ✅ 成熟稳定 | ✅ 编译时检查 | ✅ 事务支持 | ✅ 适合复杂查询 | ✅ | 最活跃 | 中等 | ✅ 优秀 | ⭐ | ⭐ | ⭐ | ⭐ | ✅ 成熟稳定 | ✅ 独立数据库 | ✅ 内置 | 适合嵌入式 | ✅ 性能优秀 | ⭐⭐ | ⭐ | ⭐ | 静态部署 | ✅ 简单 | ⭐ | 适合分析场景 | ⭐ | 时序扩展 | ⚠ 需要专业运维 | ⭐⭐ | ⭐ | ⭐ | ⚠ 架构复杂 | ⚠ 需要迁移 | ⚠ 写入性能瓶颈 | ⚠ SQLite 无法扩展到多节点
| ❌ 不支持 | ❌ 不支持            |
| ❌ 无分布式        | ❌ 单文件 | ❌ 不支持       |
| ✅ 支持           | ✅ 支持              | ✅ 支持              | ✅ 支持              | ✅ 支持              | ✅ 支持           | ✅ 支持           | ✅ 支持           | ✅ 支持             |
| ✅ 支持             | ✅ 支持              | ✅ 支持             |
| ✅ 优秀            | ✅ 优秀              | ✅ 优秀              | ✅ 优秀              | ✅ 优秀             |
| ✅ 优秀           | ✅ 优秀            |
| ✅ 优秀                | ✅ 优秀                | ✅ 优秀             |
| ✅ 优秀              | ✅ 优秀             |
| ⭐⭐            | ⭐⭐              | ⭐⭐              | ⭐⭐              | ⭐⭐             |
| ⭐⭐            | ⭐⭐              | ⭐⭐           | ⭐⭐             |
| ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ |

| ⚠ | ⚠ | ⚠ | ⚠ | ⚠ | ⚠ | ⚠ | ⚠ | ⚠ | ⚠ |
| ❌ 依赖下载，依赖 sqlx-cli 或 diesel-cli | ❌ 不支持             | ❌ 不支持        | ❌ 不支持               | ❌ 不支持                  | ❌ 不支持           | ❌ 不支持           | ❌ 不支持             |
| ❌ 不支持              | ❌ 不支持             |
| ✅ 宅主机部署 | ✅ Docker 镜像 | ✅ Docker/K8s | ✅ Home Lab/云服务器 | ✅ $5-10/月 | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ |
| ✅ 本地文件系统 | ✅ 单文件 | ✅ 无需额外服务 | ✅ 适合边缘/嵌入式 | ✅ 静态部署 | ✅ $0-5/月 | ✅ 低 | ✅ 零运维成本 | ✅ 最低 | ⭐ | ⭐ | ⭐ | ⭐ | ⭐ | ⭐ | ⭐ | ⭐ |
| ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ |
| ✅ 性能 | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ |
| ✅ 时间序列查询 | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ |
| ✅ 成熟度 | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ | ⭐⭐ |

## 4. SQLite 优化分析

### 已实施的优化

当前系统已实施了以下优化措施:

1. **WAL 模式**: Write-Ahead Logging，写入性能提升 30-50%
2. **PRAGMA 优化**:
   - `synchronous=NORMAL`: 平衡安全与性能
   - `cache_size=-64000`: 64MB 页缓存
   - `mmap_size=268435456`: 256MB 内存映射
   - `wal_autocheckpoint=1000`: 减少检查点开销
   - `busy_timeout=5000`: 5秒锁等待超时
   - `temp_store=MEMORY`: 临时表内存化

3. **索引策略**:
   - `idx_query_log_time`: 时间范围查询
   - `idx_query_log_status_time`: 阻止状态+时间
   - `idx_query_log_client_time`: 客户端+时间
   - `idx_query_log_elapsed`: 慢查询排序
   - 部分索引: `status = 'blocked'`, `status = 'error'`

4. **写入优化**:
   - 批量写入: 500 条/批次
   - 异步写入: 2 秒间隔
   - 单事务写入: 减少事务开销
   - Channel 背压: 32K 容量， 防止 OOM

   - 连接池: 20 连接

### 剩余优化空间

| 优化项 | 当前值 | 潜在收益 | 风险 |
|------|------|--------------|------|
| `busy_timeout` | 5s | 高并发写入时减少 `SQLITE_BUSY` 错误 | 可能只是掩盖问题 |
| `cache_size` | 64MB | 内存充足时有效，内存紧张时可能被换出 |
| `wal_autocheckpoint` | 1000 | 较低值可能不够频繁，导致 WAL 文件增长 |
| `mmap_size` | 256MB | 大数据集时减少磁盘 I/O | 需要足够的虚拟内存 |
| `temp_store` | MEMORY | 减少临时表 I/O | 对内存有限的环境可能增加压力 |
| 单文件数据库 | - | WAL 文件增长 | 检查点文件增长 | 不支持并发读取 | 高并发写入时可能成为瓶颈 |
| 分区策略 | - | 不支持 | 数据增长后查询性能下降，运维复杂度增加 |
| 并发写入 | - | 单写多读场景 | 高并发写入时读取性能受影响 | 需要读写分离或备份策略 |
| 缺乏高级特性 | - | 无窗口函数 | - | 无物化视图 | - | 无原生时序功能 | - | 复制需要应用层逻辑 |

### 进一步优化建议

1. **数据分区**:
   ```sql
   -- 按月分区 query_log 表
   CREATE TABLE query_log_2026_01 (
       id INTEGER PRIMARY KEY,
       time TEXT NOT NULL,
       client_ip TEXT NOT NULL,
       question TEXT NOT NULL,
       -- ... 其他字段
   );

   -- 创建历史表用于存档旧数据
   CREATE TABLE query_log_archive_2024_01 (
       LIKE query_log
   );

   -- 定时任务将活跃数据迁移到归档表
   -- 保留最近 30 天在主表，   ```

2 **索引优化**:
   - 跻加覆盖索引 `(status, time, question)` 减少按域名分组查询
   - 考虑 BRIN 索引（针对查询优化器场景)

3. **查询优化**:
   - 预计算聚合: 为 Top N 应用/域名创建物化视图
   - 定期汇总: 每小时/每天/每周统计存入汇总表
   - 异常检测批处理: 使用流式处理或窗口函数检测异常

4. **定期清理**:
   - 自动删除超过保留期的日志
   - VACUUM 操作清理碎片

## 5. 替代方案评估

### 5.1 PostgreSQL

**推荐指数: ⭐⭐⭐⭐⭐**

**核心优势**:
- **强大的并发能力**: 真正的 MVCC， 支持高并发读写
- **成熟的 Rust 生态**: sqlx, diesel, sea-orm 都有优秀支持
- **丰富的数据类型**: JSONB, 数组, 时间类型等
- **索引能力强大**: B-tree, GIN, BRIN 索引, 部分索引等
- **扩展性**: 支持分区、复制、流复制等高级特性

**Rust 生态支持**:
| 库 | 特点 | 成熟度 | 性能 | 功能 |
|------|------|------|------|------|
| sqlx | 编译时检查、异步、 优秀 | ⭐⭐⭐⭐⭐ |
| diesel | 编译时检查、同步 | 优秀 | ⭐⭐⭐⭐ |
| sea-orm | 运行时检查、异步 | 良好 | ⭐⭐⭐⭐ |

**迁移复杂度**:
- Schema 变更: SQLite TEXT 类型需转换为 PostgreSQL 类型
  - `TEXT` → `TIMESTAMPTZ` / `VARCHAR`
  - `INTEGER` → `BIGINT` / `INTEGER`
  - `datetime('now', ...)` → `NOW() - INTERVAL` 语法
- 索引语法: 大部分兼容，部分需调整
- RETURNING 子句: SQLite 不支持，需重写为游标查询
- 连接字符串: 从文件路径改为连接字符串

**部署方案**:
- **Docker**: 安️ postgres:15-alpine 容器
- **Kubernetes**: 使用 StatefulSet 部署
- **云服务**: AWS RDS, GCP Cloud SQL, Azure Database

**运维成本**:
- **服务器**: 需要独立 VM 或托管服务
- **内存**: 建议至少 2GB
- **存储**: SSD 推荐
- **备份**: 黺议流复制
- **监控**: Prometheus + Grafana

### 5.2 MySQL/MariaDB

**推荐指数: ⭐⭐⭐⭐

**优势**:
- 广泛部署: 几乎所有云服务商支持
- 熟悉度高: 运维工具丰富
- 复制能力强: 主从复制简单

**劣势**:
- 并发性能: 不如 PostgreSQL
- JSON 支持: 原生支持较弱
- 高级特性: 相对较少

**Rust 生态支持**:
| 库 | 支持度 |
|------|------|
| sqlx | ⭐⭐⭐⭐ |
| diesel | ⭐⭐⭐ |
| sea-orm | ⭐⭐⭐⭐ |

### 5.3 TimescaleDB (PostgreSQL 扩展)

**推荐指数: ⭐⭐⭐⭐⭐

**优势**:
- 时序优化: 自动分区、保留策略、压缩
- 查询性能: 时序查询极快
- 兼容性: 完全兼容 PostgreSQL
- 成熟度: 基于 PostgreSQL, 生态成熟

**适用场景**:
- 大规模时序数据: 适合 TB 级别数据
- 需要保留策略: 自动管理历史数据
- 复杂时序查询: 聚合分析

**Rust 生态**:
- 官方支持: 通过 sqlx postgres feature
- 迁移: 从 SQLite 迁移简单

**部署成本**:
- 需要 PostgreSQL 基础设施
- 额外配置: TimescaleDB 扩展

### 5.4 ClickHouse

**推荐指数: ⭐⭐⭐

**优势**:
- 分析性能: OLAP 查询极快
- 列式存储: 娱乐级压缩效率高
- 实时分析: 适合实时仪表盘

**劣势**:
- 运维复杂度: 需要专业团队
- 学习曲线: SQL 语法不同
- 适合场景: 不适合事务处理

**Rust 生态**:
- 官方驱动: clickhouse crate (较新)
- ORM 支持: 较弱

**部署**:
- 需要集群部署
- 资源消耗: 较大

### 5.5 RocksDB (嵌入式 KV 存储)

**推荐指数: ⭐⭐

**优势**:
- 嵌入式: 无需独立服务
- 性能: 读写性能优秀
- 简单: 运维成本低

**劣势**:
- 查询能力: 不支持复杂分析
- 单机: 无分布式能力
- 生态: Rust 绑定相对较新

**Rust 生态**:
- rust-rocksdb: 官方绑定

**适用场景**:
- 边缘设备
- 嵌入式系统
- 对查询复杂度要求低的场景

---

## 6. 迁移路径建议

### 短期路径: SQLite 优化

**适用条件**:
- 数据量增长可控(日增量 < 10 万条)
- 主要是时间范围查询
- 单机部署可接受
- 运维资源有限

**实施步骤**:
1. 添加数据分区(按月)
2. 优化索引策略
3. 实现数据归档
4. 添加预计算聚合表
5. 监控数据库性能指标

**代码变更**:
```rust
// 添加分区配置
pub struct PartitionConfig {
    pub enable_monthly_partition: bool,
    pub archive_table_name: String,
}

// 添加归档任务
pub async fn archive_old_data(db: &DbPool, config: &PartitionConfig) {
    // 实现归档逻辑
}
```

**预期效果**:
- 主表保持在 1GB 以下
- 查询性能提升 50%+
- 可追溯历史数据

### 中期路径: PostgreSQL

**适用条件**:
- 数据量快速增长(日增量 > 10 万条)
- 需要高可用性
- 有运维能力

**实施步骤**:
1. 设计迁移方案
2. 实现双写支持(Schema 变更)
3. 部署 PostgreSQL
4. 迁移数据
5. 更新连接配置

6. 切换应用连接

**代码变更**:
```rust
// Cargo.toml
sqlx = { version = "0.8", features = ["postgres", "runtime-tokio-rustls", "migrate", "chrono", "uuid"] }

// 配置
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
}

```

**迁移计划**:
- **阶段 1**: 准备
  - 创建归档表
  - 实现数据迁移脚本
  - 测试迁移
- **阶段 2**: 执行
  - 低峰期执行(维护窗口)
  - 启动双写支持
  - 监控数据同步
- **阶段 3**: 完成切换
  - 更新应用配置
  - 切换数据库连接
  - 验证功能

**预期效果**:
- 支持无限扩展
- 查询性能提升 5-10x
- 高可用性

### 长期路径: TimescaleDB

**适用条件**:
- 需要高级时序分析
- 数据量 > 10GB
- 有 PostgreSQL 运维经验

**实施步骤**:
1. 部署 PostgreSQL
2. 启用 TimescaleDB 扩展
3. 创建 hypertable
4. 配置保留策略

**代码变更**:
```sql
-- 启用 TimescaleDB 扩展
CREATE EXTENSION IF NOT EXISTS timescaledb;

-- 将 query_log 转换为 hypertable
SELECT create_hypertable('query_log', 'time');

-- 配置保留策略(保留 90 天)
SELECT add_retention_policy('query_log', INTERVAL '90 day');
```

**预期效果**:
- 自动数据分区
- 查询性能大幅提升
- 存储空间优化

---

## 7. 推荐方案

### 当前阶段推荐: SQLite 优化

**理由**:
1. **数据规模尚小**: 1.4GB 对 SQLite 来说完全可控
2. **优化空间大**: 当前优化措施还有提升空间
3. **零运维成本**: 无需额外服务
4. **迁移风险低**: 保持现有架构

**具体建议**:
1. 实现按月分区
2. 添加数据归档机制
3. 优化索引策略
4. 监控数据库性能

### 增长触发条件

当出现以下情况时，考虑迁移到 PostgreSQL:

1. **数据量**: 日增量 > 10 万条, 总量 > 10GB
2. **并发**: 需要多实例部署
3. **查询复杂度**: 需要复杂分析查询
4. **可用性要求**: 需要 99.9% SLA

### 长期方案

**PostgreSQL + TimescaleDB**

适合以下场景:
- 企业级部署
- 多租户需求
- 复杂分析需求
- 合规要求(数据保留)

---

## 8. Rust 生态详细评估

### sqlx (推荐)

**优势**:
- 编译时 SQL 检查
- 异步原生支持
- 多数据库支持
- 活跃维护

**使用示例**:
```rust
use sqlx::postgres::PgPoolOptions;

let pool = PgPoolOptions::new()
    .max_connections(20)
    .connect(&database_url)
    .await?;

// 编译时检查的查询
let logs = sqlx::query_as!(QueryLog,
    "SELECT * FROM query_log WHERE time >= $1"
    .bind(start_time)
    .fetch_all(&pool)
    .await?;
```

**迁移成本**: 低(与 SQLite API 相似)

### Diesel

**优势**:
- 类型安全
- 编译时检查
- 成熟稳定

**劣势**:
- 同步 API
- 宏使用较多

**迁移成本**: 中等(需要重写查询)

### sea-orm

**优势**:
- 动态查询
- 活跃社区
- 多数据库支持

**劣势**:
- 运行时检查
- 性能开销

**迁移成本**: 中等(需要重写查询层)

---

## 9. 冨策框架

```
┌─────────────┐
│   数据规模   │
└──────┬──────┘
       │
       ▼
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   < 5GB     │────▶│   < 50GB    │────▶│   > 50GB    │
└─────────────┘     └─────────────┘     └─────────────┘
       │                     │                     │
       ▼                     ▼                     ▼
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   SQLite    │     │ PostgreSQL  │     │TimescaleDB  │
│   (优化版)  │     │             │     │             │
└─────────────┘     └─────────────┘     └─────────────┘
```

**决策建议**:
- **当前(1.4GB)**: 继续使用 SQLite, 实施优化方案
- **增长到 10GB**: 评估 PostgreSQL 迁移
- **增长到 50GB+**: 考虑 TimescaleDB

---

## 10. 附录

### 监控指标

```sql
-- 数据库大小
SELECT page_count * page_size as size FROM pragma_page_count;

-- 表大小
SELECT name, pgsize FROM dbstat WHERE name = 'query_log';

-- 索引使用情况
SELECT * FROM pragma_index_info WHERE name LIKE 'idx_query_log%';

-- WAL 文件大小
SELECT * FROM pragma_wal_checkpoint;
```

### 性能测试建议

```bash
# 写入性能
sysbench oltp_write_only --db-driver=sqlite --tables=1 \
  --sqlite-db=/path/to/dns.db prepare

# 查询性能
sysbench oltp_read_only --db-driver=sqlite --tables=1 \
  --sqlite-db=/path/to/dns.db run
```

### 备份策略

```bash
# SQLite 备份(热备份)
sqlite3 /path/to/dns.db ".backup 'backup.sql'"

# 完整备份(需要停机)
sqlite3 /path/to/dns.db ".backup backup.sql"
```

---

## 11. 总结

**当前建议**: 继续使用 SQLite, 实施优化方案

**理由**:
1. 数据规模(1.4GB)完全在 SQLite 能力范围内
2. 已有优化措施(WAL, 批量写入)表现良好
3. 零运维成本, 适合独立开发者
4. 迁移风险低, 保持系统稳定

**下一步行动**:
1. 实现按月分区
2. 添加数据归档机制
3. 优化索引策略
4. 建立性能监控

**未来规划**:
- 数据量 > 10GB 时评估 PostgreSQL 迁移
- 需要 TimescaleDB 时先启用扩展
- 保持 sqlx 作为数据库抽象层

---

**报告完成日期**: 2026-03-09
**作者**: research-thompson
