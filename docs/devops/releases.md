# rust-dns-backend Release History

## v1.6.0 — 2026-03-04

**主题：Query Log 高级过滤**

### 变更内容

| 类型 | 描述 |
|------|------|
| feat | 新增 `qtype` 查询参数 — 支持按 DNS 记录类型过滤 query log（A、AAAA、CNAME、MX 等） |
| feat | 新增 `time_range` 查询参数 — 支持快速时间窗口过滤（1h、24h、7d），无需手动传起止时间戳 |

### Tag 信息

| 项目 | 值 |
|------|-----|
| Tag | `v1.6.0` (annotated) |
| Commit | `8c1f834` |
| Remote | `github.com:EmotionalAmo/rust-dns-backend.git` |
| Push 状态 | 成功 |

### 对应 Frontend 版本

`rust-dns-frontend v1.6.0` — 时间范围按钮组、QTYPE 下拉过滤框、域名搜索自动补全。

---

## v1.5.0 — 2026-03-04

**主题：上游趋势数据修复**

### 变更内容

| 类型 | 描述 |
|------|------|
| fix | `get_upstream_trend` 聚合容器由 `HashMap` 改为 `BTreeMap`，修复时间轴乱序问题 |

### Tag 信息

| 项目 | 值 |
|------|-----|
| Tag | `v1.5.0` (annotated) |
| Remote | `github.com:EmotionalAmo/rust-dns-backend.git` |

---

## v1.2.0 — 2026-03-03

**主题：TCP Upstream 支持**

| 类型 | 描述 |
|------|------|
| feat | TCP upstream 支持（`tcp://` 前缀配置） |
| fix | DoH/DoT upstream 健康检查修复 |
| fix | DoT upstream 连通性测试修复 |

---

## v1.1.0 — 2026-03-03

**主题：DoT/DoH Upstream、Audit Middleware**

| 类型 | 描述 |
|------|------|
| feat | DNS-over-TLS (DoT) upstream 支持 |
| feat | DNS-over-HTTPS (DoH) upstream 支持 |
| feat | Audit middleware — 自动记录所有写操作 |
