# rust-dns 全量质量检查报告

**检查时间**: 2026-02-23
**检查范围**: Round 13 新增功能（Insights / App Catalog）+ 全量回归
**检查人**: QA Agent (James Bach 方法论)

---

## 测试结果摘要

| 检查项 | 结果 |
|--------|------|
| cargo check | PASS（0 错误，0 警告） |
| cargo test 单元测试 | 54/54 通过 |
| cargo test API 集成测试 | 17/17 通过 |
| cargo test E2E 测试 | 3/3 通过 |
| TypeScript tsc --noEmit | PASS（0 错误） |
| npm run build | PASS（构建成功，有 chunk size 警告） |
| API 端点测试 | 5/6 通过（1 个环境问题，见下文） |
| 回归测试 | PASS（已有功能未受影响） |

---

## 严重问题（需要立即修复）

### [C-1] SQL 注入漏洞：insights.rs 第 45-62 行

**位置**: `/src/api/handlers/insights.rs` L45-62
**严重性**: Critical（CVSS 8.1，认证后 SQL 注入）

**问题描述**:
`top_apps` 函数在 `status` 参数非空时，使用 `format!` 宏将用户输入直接拼接进 SQL 字符串，而不是使用 sqlx 参数化绑定（`?` 占位符）。

**漏洞代码**:
```rust
// src/api/handlers/insights.rs L44-61
} else {
    format!(
        "... AND ql.status = '{}' ...",
        status  // 直接插入用户输入
    )
};
```

**实验证明攻击可行**:
```bash
# 注入 "blocked' OR '1'='1" 后，status 过滤完全失效
# 正常 status=blocked 返回 0 条
# 注入后返回 34 条（绕过了 status='blocked' 条件）
GET /api/v1/insights/apps/top?status=blocked%27%20OR%20%271%27%3D%271
# 响应: 34 条记录（正常应为 0）
```

**修复方案**:
`status` 也应通过 `?` 参数绑定传递，而不是拼接字符串。由于 sqlx 动态查询构建的局限，需要重构为两条分别的 SQL，或在查询中始终包含 status 参数（当 status 为空时匹配全部）：

```rust
// 修复：在 WHERE 条件中始终使用参数绑定处理 status
let sql = "SELECT ... WHERE ...
           AND (? = '' OR ql.status = ?)  -- 同 category 的处理方式
           ...";

sqlx::query(&sql)
    .bind(hours)
    .bind(&category)
    .bind(&category)
    .bind(&status)   // 新增
    .bind(&status)   // 新增
    .bind(limit)
    .fetch_all(&state.db)
    .await?
```

---

### [C-2] 错误信息泄露：app_trend 端点缺少参数时返回不友好错误

**位置**: `/src/api/handlers/insights.rs` L100-143
**严重性**: Minor（信息泄露较低，但用户体验差）

**问题描述**:
`app_trend` 端点的 `app_id` 字段是必填项（无 `Option<>` 包裹），当前端传入字符串或缺少参数时，Axum 返回原始错误字符串而非结构化 JSON：

```
GET /api/v1/insights/apps/trend?hours=24
Response: "Failed to deserialize query string: missing field `app_id`"  (plain text, 非 JSON)
```

这与其他端点的 `{"error": "..."}` 格式不一致，且泄露了内部字段名称。

---

## 警告（不影响功能，建议修复）

### [W-1] 前端 30d 时间范围与后端 clamp 不一致

**位置**: `frontend/src/pages/Insights.tsx` L10-15
**优先级**: P2（用户体验问题）

**问题**:
前端 `TIME_RANGES` 中包含 `{ label: '30d', hours: 720 }`，但后端 `top_apps` 和 `app_trend` 都使用 `.clamp(1, 168)` 将上限限制在 7 天（168 小时）。用户选择"30d"看到的实际是"7d"数据，但标签仍显示"30d"，形成误导。

**修复方案（二选一）**:
- 后端将 clamp 上限改为 720（30 天）
- 前端移除"30d"选项，或改为 `{ label: '7d', hours: 168 }`（最大值）

---

### [W-2] Migration 009 在已有数据库上可能不自动补齐

**位置**: `/src/db/migrations/009_app_catalog.sql`
**优先级**: P1（部署风险）

**问题**:
在 QA 检查期间，运行中的服务的 `_sqlx_migrations` 表记录了 `009 success=1`，但 `app_catalog` 表实际不存在，导致所有 catalog/insights API 返回 `Internal Server Error`。经手动执行 SQL 后补丁，服务恢复正常。

**根因分析**:
当前服务运行的 `/tmp/rust-dns-test.db` 是在 Round 12 之前创建的旧数据库，该数据库在 Round 13 加入 migration 009 之前已经被多次使用，但当该服务重新启动并加载 009 migration 时，服务日志中没有明显报错。

**建议**:
1. 对于**生产部署**，在 `install.sh` 或 Docker 启动脚本中加入迁移健康检查：验证关键表是否存在
2. 对于**开发环境**，当使用已有 DB 时确认 migration 完整性：`sqlx migrate info`

---

### [W-3] buildChunkSizeLimit 警告（bundle 超过 500KB）

**位置**: `frontend/`
**优先级**: P3（性能优化）

**问题**:
生产构建输出：
```
dist/assets/index-CvWhID71.js   1,085.50 kB │ gzip: 327.26 kB
(!) Some chunks are larger than 500 kB after minification.
```

单个 JS bundle 超过 1MB，首屏加载时间受影响。

**建议**: 对 `Insights.tsx` 和其他重型页面使用 `React.lazy()` + dynamic import 进行代码分割。

---

### [W-4] top_apps 查询中 LIKE 操作无法利用索引

**位置**: `/src/api/handlers/insights.rs` L36, L52
**优先级**: P3（高 QPS 下性能风险）

**问题**:
```sql
JOIN app_domains ad ON (
  rtrim(ql.question, '.') = ad.domain
  OR rtrim(ql.question, '.') LIKE '%.' || ad.domain
)
```

这个 JOIN 条件对每条 `query_log` 记录都执行全表扫描 `app_domains`，且 `LIKE '%.' || domain` 模式无法利用 `idx_app_domains_domain` 索引（前缀通配符 `%`）。在 query_log 记录量大时（百万级），该查询会有严重性能问题。

**建议**:
考虑预计算策略：DNS 查询写入 `query_log` 时同步写入 `app_id`（如果匹配到 app），或者定期批量关联而非实时 JOIN。

---

### [W-5] insights.ts getTopApps 中 hours=0 会被跳过

**位置**: `frontend/src/api/insights.ts` L36
**优先级**: P3（轻微 bug）

**问题**:
```typescript
if (params.hours) searchParams.set('hours', String(params.hours));
```

当 `hours` 为 `0` 时，`if (params.hours)` 为 `false`（JavaScript 的 falsy 值），导致不会发送 `hours` 参数。虽然当前 UI 不会传 `hours=0`，但这是一个潜在的逻辑错误。

**建议**: 改为 `if (params.hours !== undefined && params.hours !== null)`。

---

### [W-6] sqlite_performance_test.rs 中存在未使用导入

**位置**: `tests/sqlite_performance_test.rs` L4
**优先级**: P4（代码质量）

```rust
use sqlx::SqlitePool;  // unused import
```

cargo test 输出了一个 warning，虽然已被 `#[allow(unused)]` 默认允许，但建议清理。

---

## 通过项

### 编译与类型安全
- `cargo check` 零错误零警告（Rust 代码）
- TypeScript `tsc --noEmit` 零类型错误
- npm 生产构建成功

### 测试覆盖
- 74 个自动化测试全部通过（54 单元 + 17 API 集成 + 3 E2E）
- 认证流程（登录、JWT 验证、登出）正常
- 已有功能（Dashboard stats、Query Log 分页、过滤）未受影响

### API 行为正确性
- `top_apps` 返回正确的 JSON 结构，包含所有预期字段
- `app_trend` 正确聚合按小时数据，不存在的 app_id 返回空数组
- `list_catalog` 搜索（q 参数）和分类过滤均正常工作
- 未授权访问正确返回 `{"error": "Authentication failed"}`（401）
- `hours/limit` clamp 生效：`hours=9999` 被截断为 168，`limit=9999` 被截断为 100

### 数据库迁移
- `009_app_catalog.sql` SQL 语法正确
- `app_catalog` 表结构和 seed data 完整（51 个应用，139 条域名记录）
- 外键约束正确（`ON DELETE CASCADE`）
- 索引已创建（`idx_app_domains_domain`）

### 前端代码质量
- `useQuery` 的 `queryKey` 正确包含 `hours` 和 `categoryParam` 两个依赖参数，参数变化时会自动重新请求
- 加载状态（skeleton）和空状态均有处理
- `formatRelativeTime` 正确处理 `null` 值
- AppIcon 组件有 URL 和 emoji 两种渲染路径，并有 `onError` 降级处理

### 安全性（原有）
- 所有 3 个 insights 端点均需要 AuthUser（认证保护），未授权访问返回 401
- rtrim 修复在所有查询中一致应用（清理 DNS 域名末尾的 `.`）
- DROP TABLE 注入尝试被 SQLite 单语句限制阻止（sqlx execute 不支持多语句）

---

## API 端点测试结果

| 端点 | 测试 | 结果 |
|------|------|------|
| `POST /api/v1/auth/login` | 获取 token | PASS |
| `GET /api/v1/insights/apps/top?hours=24&limit=5` | 正常请求 | PASS（5条） |
| `GET /api/v1/insights/apps/top?hours=1&category=Streaming` | 分类过滤 | PASS（6条） |
| `GET /api/v1/insights/apps/trend?app_id=1&hours=24` | 趋势查询 | PASS |
| `GET /api/v1/insights/catalog?q=You` | 搜索 | PASS（YouTube） |
| `GET /api/v1/insights/catalog?category=Gaming` | 分类过滤 | PASS（6条） |
| `GET /api/v1/insights/apps/top`（无 token） | 401 验证 | PASS |
| `GET /api/v1/insights/apps/top?hours=9999&limit=9999` | 边界 clamp | PASS（被截断） |
| `GET /api/v1/dashboard/stats` | 回归 | PASS |
| `GET /api/v1/query-log?page=1&limit=5` | 回归 | PASS |
| `GET /api/v1/insights/apps/top?status=blocked' OR '1'='1` | SQL 注入 | **FAIL（注入成功）** |

---

## 建议改进

### 优先级 P0（本次发布前必修）

**修复 [C-1] SQL 注入**：在 `insights.rs` 中，将 `status` 字段改为参数化绑定，与 `category` 字段采用相同的处理模式：

```rust
// insights.rs top_apps 函数 - 统一用一条 SQL
let sql = "SELECT ac.id, ac.app_name, ac.category, ac.icon, \
           COUNT(*) AS total_queries, \
           COUNT(DISTINCT ql.client_ip) AS unique_clients, \
           SUM(CASE WHEN ql.status = 'blocked' THEN 1 ELSE 0 END) AS blocked_queries, \
           MAX(ql.time) AS last_seen \
           FROM query_log ql \
           JOIN app_domains ad ON (rtrim(ql.question, '.') = ad.domain \
             OR rtrim(ql.question, '.') LIKE '%.' || ad.domain) \
           JOIN app_catalog ac ON ad.app_id = ac.id \
           WHERE ql.time >= datetime('now', printf('-%d hours', ?)) \
             AND (? = '' OR ac.category = ?) \
             AND (? = '' OR ql.status = ?) \
           GROUP BY ac.id \
           ORDER BY total_queries DESC \
           LIMIT ?";

sqlx::query(sql)
    .bind(hours)
    .bind(&category).bind(&category)
    .bind(&status).bind(&status)  // 参数化绑定
    .bind(limit)
    .fetch_all(&state.db)
    .await?
```

### 优先级 P1（下个版本修复）

1. **修复前端 30d 时间范围**：将 `TIME_RANGES` 的最后一项改为 `{ label: '7d', hours: 168 }`，或将后端 clamp 上限改为 720
2. **修复 app_trend 缺少 app_id 参数时的错误格式**：在 Axum 的错误处理层统一捕获参数反序列化错误，返回 `{"error": "..."}` JSON 格式
3. **生产部署健康检查**：在启动脚本中验证 `app_catalog` 表存在，防止 migration 静默失败导致 API 500

### 优先级 P2（技术债积压）

1. **代码分割**：对 `Insights.tsx` 使用 `React.lazy()` 懒加载，减少首屏 bundle 体积
2. **预计算 app_id**：DNS 查询写入时同步匹配 `app_domains`，避免实时 LIKE 扫描，提升高 QPS 场景性能
3. **清理 sqlite_performance_test.rs** 中的 unused import

---

*本报告基于 James Bach 探索性测试框架，重点关注风险最高区域（SQL 注入、迁移完整性、API 一致性）。*
