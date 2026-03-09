# ADR-001: 数据库架构决策记录

## 元信息

| 项目 | 内容 |
|------|------|
| **状态** | 提议中 (Proposed) |
| **日期** | 2026-03-09 |
| **决策者** | CTO (Werner Vogels 思维模型) |
| **影响范围** | 数据存储层、查询性能、运维复杂度 |

---

## 背景 (Context)

### 当前状态

- **数据库**: SQLite 3.x + sqlx
- **数据规模**: 107MB (活跃) + 1.3GB (历史积累)
- **设备数**: 14 台
- **写入模式**: 批量异步写入 (500条/批, 2秒间隔)
- **索引**: 已优化 (10+ 复合/部分索引)

### 人类反馈

> SQLite 已不足以支撑使用

### 代码分析发现

#### 1. 数据模型 (7 张核心表)

```
users                 -- 用户认证 (小表, <100行)
filter_lists          -- 过滤列表订阅 (小表, <50行)
custom_rules          -- 自定义规则 (中等表, ~1万行)
dns_rewrites          -- DNS 重写 (小表)
clients               -- 客户端配置 (小表, 14行)
query_log             -- 查询日志 (大表, 百万级) <-- 问题核心
audit_log             -- 审计日志 (中等表)
app_catalog           -- 应用目录 (中等表, ~5000行)
```

#### 2. query_log 表特征

```sql
CREATE TABLE query_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    time        TEXT NOT NULL,
    client_ip   TEXT NOT NULL,
    question    TEXT NOT NULL,
    qtype       TEXT NOT NULL,
    answer      TEXT,
    status      TEXT NOT NULL,
    reason      TEXT,
    upstream    TEXT,
    elapsed_ns  INTEGER,
    upstream_ns INTEGER,
    app_id      INTEGER
);
```

- **写入频率**: 每次DNS查询产生1条记录
- **写入优化**: 已实现批量异步写入 (32K channel buffer)
- **查询模式**:
  - 时间范围过滤 (`time >= datetime('now', '-24 hours')`)
  - 状态过滤 (`status = 'blocked'`)
  - 客户端过滤 (`client_ip LIKE ?`)
  - 域名模糊搜索 (`question LIKE ?`)
  - 聚合统计 (COUNT, GROUP BY)

#### 3. 现有 PRAGMA 优化

```rust
PRAGMA journal_mode=WAL          -- 写前日志
PRAGMA synchronous=NORMAL        -- 平衡安全与性能
PRAGMA cache_size=-64000         -- 64MB 缓存
PRAGMA mmap_size=268435456       -- 256MB mmap
PRAGMA wal_autocheckpoint=1000   -- WAL 检查点
PRAGMA busy_timeout = 5000       -- 5s 锁等待
PRAGMA temp_store = MEMORY       -- 内存临时表
```

---

## 问题诊断

### SQLite 的具体瓶颈

基于代码分析和数据规模，问题不在单一维度，而是**组合因素**:

| 瓶颈类型 | 具体表现 | 严重程度 |
|----------|----------|----------|
| **数据量** | 1.4GB 总量，query_log 占比 >90% | 高 |
| **索引膨胀** | 10+ 索引导致写入放大 | 中 |
| **并发写入** | WAL 模式单写者，批量写入时阻塞读取 | 中 |
| **COUNT(*) 性能** | 分页查询需要全表计数 | 高 |
| **LIKE 查询** | `question LIKE '%keyword%'` 无法用索引 | 高 |
| **聚合统计** | Dashboard/Insights 需要扫描大量数据 | 高 |

### 根因分析

```
数据流:
DNS Query -> [hot path: 内存过滤/缓存] -> [async: 批量写入 SQLite]
                                              |
                                              v
API Query -> [SQLite 全表扫描/聚合] -> 响应
                    ^
                    |
              这里是瓶颈
```

**核心问题**: query_log 表承担了两个冲突的职责:
1. **时序数据存储** (高频追加写)
2. **分析查询** (复杂聚合/过滤)

SQLite 不是为这种混合负载设计的。

---

## 架构方案评估

### 方案 A: 继续用 SQLite + 优化

**思路**: 深度优化现有架构

**优化项**:
1. 定期 VACUUM + 清理历史数据
2. 增加查询缓存层 (Redis/moka)
3. 预计算统计物化视图
4. 限制 query_log 保留天数 (7天)

**优点**:
- 零迁移成本
- 运维简单
- 适合单机部署

**缺点**:
- 治标不治本
- LIKE 查询无解
- 聚合查询仍然慢
- 数据增长后问题复现

**适用场景**: 数据量 <500MB, QPS <1000

**结论**: 不推荐。1.4GB 已超出 SQLite 舒适区。

---

### 方案 B: 迁移到 PostgreSQL (推荐)

**思路**: 用成熟的关系型数据库替代 SQLite

**架构变化**:
```
Before:  App -> SQLite (单文件)
After:   App -> PostgreSQL (独立进程/托管服务)
```

**优点**:
- 真正的并发 (MVCC)
- 更强的查询优化器
- 原生支持分区表
- 丰富的索引类型 (GIN, BRIN)
- 成熟的运维生态

**缺点**:
- 需要额外部署/运维
- 增加系统复杂度
- 迁移有风险

**适用场景**: 数据量 1GB-10TB, 需要长期扩展

**结论**: 推荐，但需分阶段实施。

---

### 方案 C: 混合架构 - 日志分离 (最推荐)

**思路**: 核心配置保留 SQLite，日志类数据迁移到专用存储

**架构设计**:
```
                    +------------------+
                    |   rust-dns       |
                    +--------+---------+
                             |
            +----------------+----------------+
            |                                 |
    +-------v-------+                +--------v--------+
    |    SQLite     |                |   PostgreSQL    |
    |   (配置数据)   |                |   (query_log)   |
    +---------------+                +-----------------+
    | users         |                | query_log       |
    | filter_lists  |                | audit_log       |
    | custom_rules  |                | (分区表)         |
    | clients       |                +-----------------+
    | settings      |
    +---------------+
```

**数据分离策略**:

| 表名 | 存储位置 | 理由 |
|------|----------|------|
| users | SQLite | 小表，低频变更 |
| filter_lists | SQLite | 小表，低频变更 |
| custom_rules | SQLite | 中等表，可接受 |
| dns_rewrites | SQLite | 小表 |
| clients | SQLite | 小表 |
| client_groups | SQLite | 小表 |
| **query_log** | **PostgreSQL** | 大表，高频写入+复杂查询 |
| **audit_log** | **PostgreSQL** | 中等表，持续增长 |

**优点**:
- 渐进式迁移，风险可控
- 核心功能不受影响
- 解决主要性能瓶颈
- 保留 SQLite 的简单性 (配置管理)

**缺点**:
- 双数据库连接管理
- 跨库查询需要应用层 JOIN
- 代码改动量中等

**结论**: 最佳平衡方案。

---

### 方案 D: 时序数据库 (ClickHouse/InfluxDB)

**思路**: query_log 迁移到专用时序数据库

**优点**:
- 极致的时序查询性能
- 高压缩比
- 天然支持时间分区

**缺点**:
- 学习曲线陡峭
- 运维复杂度高
- 对 14 台设备的规模过度设计

**结论**: 不推荐。当前规模不需要。

---

## 决策

### 推荐: 方案 C - 混合架构 (SQLite + PostgreSQL)

**理由**:

1. **符合 "先垂直扩展，再水平扩展" 原则**
   - 当前问题可通过拆分数据解决，不需要全量迁移

2. **符合 "数据库是最难扩展的部分，提前规划" 原则**
   - query_log 是增长最快的表，优先处理

3. **符合 "Boring Technology" 原则**
   - PostgreSQL 是成熟稳定的选择
   - SQLite 继续用于配置存储

4. **符合 "Monolith First" 原则**
   - 保持单体应用架构
   - 只是数据库层分离

---

## 实施计划

### Phase 0: 准备工作 (1-2 天)

1. 添加 PostgreSQL 支持 (sqlx feature flag)
2. 创建数据库连接抽象层
3. 本地测试环境搭建

### Phase 1: query_log 迁移 (3-5 天)

1. 创建 PostgreSQL schema (带分区)
2. 实现双写逻辑 (同时写 SQLite 和 PostgreSQL)
3. 数据迁移脚本
4. 切换读取到 PostgreSQL
5. 停止 SQLite query_log 写入

### Phase 2: audit_log 迁移 (1-2 天)

1. 迁移 audit_log 表
2. 验证审计功能

### Phase 3: 清理与优化 (1 天)

1. 删除 SQLite 中的已迁移表
2. VACUUM SQLite
3. 性能测试

---

## PostgreSQL Schema 设计

```sql
-- query_log 表 (按月分区)
CREATE TABLE query_log (
    id          BIGSERIAL,
    time        TIMESTAMPTZ NOT NULL,
    client_ip   INET NOT NULL,
    question    TEXT NOT NULL,
    qtype       VARCHAR(16) NOT NULL,
    answer      TEXT,
    status      VARCHAR(16) NOT NULL,
    reason      TEXT,
    upstream    TEXT,
    elapsed_ns  BIGINT,
    upstream_ns BIGINT,
    app_id      INTEGER
) PARTITION BY RANGE (time);

-- 创建月度分区
CREATE TABLE query_log_2026_03 PARTITION OF query_log
    FOR VALUES FROM ('2026-03-01') TO ('2026-04-01');

-- 索引
CREATE INDEX idx_query_log_time ON query_log(time DESC);
CREATE INDEX idx_query_log_client_time ON query_log(client_ip, time DESC);
CREATE INDEX idx_query_log_status_time ON query_log(status, time DESC);
CREATE INDEX idx_query_log_question ON query_log USING gin(to_tsvector('simple', question));

-- 自动保留策略 (7天)
CREATE OR REPLACE FUNCTION cleanup_old_partitions() RETURNS void AS $$
BEGIN
    EXECUTE format('DROP TABLE IF EXISTS query_log_%s',
        to_char(current_date - interval '8 days', 'YYYY_MM'));
END;
$$ LANGUAGE plpgsql;
```

---

## 风险评估

| 风险 | 可能性 | 影响 | 缓解措施 |
|------|--------|------|----------|
| 迁移过程中数据丢失 | 低 | 高 | 双写期间保留 SQLite 作为备份 |
| PostgreSQL 连接失败 | 中 | 高 | 实现 fallback 到 SQLite (只读) |
| 性能不如预期 | 低 | 中 | 充分的基准测试 |
| 运维复杂度增加 | 高 | 中 | 使用托管服务 (Supabase/Neon/Railway) |

---

## 回滚方案

1. **Phase 1 期间**: 停止双写，继续使用 SQLite
2. **Phase 1 之后**: 导出 PostgreSQL 数据回 SQLite (接受性能下降)
3. **完全回滚**: 恢复到迁移前的 git commit

---

## 成本估算

### 自托管 PostgreSQL

| 项目 | 规格 | 月成本 |
|------|------|--------|
| VPS (2C4G) | DigitalOcean/Hetzner | $10-20 |

### 托管 PostgreSQL

| 服务商 | 规格 | 月成本 |
|--------|------|--------|
| Supabase | Free tier (500MB) | $0 |
| Neon | Free tier (3GB) | $0 |
| Railway | 1GB | $5 |
| AWS RDS | db.t3.micro | $15 |

**推荐**: 初期使用 Neon/Supabase 免费层，超出后按需升级。

---

## 后续优化方向

1. **全文搜索**: PostgreSQL tsvector + tsquery 替代 LIKE
2. **物化视图**: 预计算 Dashboard 统计
3. **连接池**: PgBouncer 减少连接开销
4. **只读副本**: 高负载场景下分离读写

---

## 参考

- [SQLite WAL Mode](https://www.sqlite.org/wal.html)
- [PostgreSQL Partitioning](https://www.postgresql.org/docs/current/ddl-partitioning.html)
- [When to use SQLite](https://www.sqlite.org/whentouse.html)
- [PostgreSQL vs SQLite](https://www.postgresql.org/about/featurematrix/)

---

## 附录: 代码改动清单

### 新增文件

```
src/db/pg/mod.rs           -- PostgreSQL 连接管理
src/db/pg/query_log.rs     -- query_log PostgreSQL 操作
src/db/pg/audit_log.rs     -- audit_log PostgreSQL 操作
```

### 修改文件

```
Cargo.toml                 -- 添加 sqlx postgres feature
src/db/mod.rs              -- 双数据库连接管理
src/db/query_log_writer.rs -- 双写逻辑
src/api/handlers/*.rs      -- 切换到 PostgreSQL 读取
src/config.rs              -- 添加 PostgreSQL 配置项
```

### 配置变更

```toml
# config.toml
[database]
type = "postgres"  # or "sqlite" for fallback
sqlite_path = "./rust-dns.db"
postgres_url = "postgres://user:pass@host/db"

[database.query_log]
storage = "postgres"  # or "sqlite"
retention_days = 7
```

---

## 下一步行动

1. **确认方案**: 与人类确认是否采用混合架构方案
2. **选择托管服务**: 评估 Neon/Supabase/AWS RDS
3. **创建迁移分支**: `git checkout -b feature/postgres-migration`
4. **开始 Phase 0**: 添加 PostgreSQL 支持

---

*Document generated by CTO Agent (Werner Vogels thinking model)*
*Date: 2026-03-09*
