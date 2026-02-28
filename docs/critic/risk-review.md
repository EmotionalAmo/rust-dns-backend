# Ent-DNS 安全与风险审视报告

**审计日期**: 2026-02-19
**审计范围**: Ent-DNS v0.1.0 完整代码库
**审计类型**: 代码安全 + 配置安全 + 架构安全

---

## 执行摘要

| 风险等级 | 数量 |
|----------|------|
| Critical | 1 |
| High | 3 |
| Medium | 4 |
| Low | 3 |

**总体评价**: Ent-DNS 具备良好的安全基础，使用了行业标准的安全库（Argon2, JWT, sqlx）。然而存在几个生产环境必须立即修复的高危和严重问题。

---

## Critical 风险 (24-48小时修复)

### 1. 默认 JWT Secret 未强制修改

**位置**: `src/config.rs:63`

```rust
.set_default("auth.jwt_secret", "change-me-in-production")?
```

**风险**: 如果用户使用默认配置运行，JWT 可被任意伪造，攻击者可获取任何角色的权限。

**缓解方案**:
- 启动时检查 JWT secret 是否为默认值，如果是则拒绝启动
- 在文档中强调必须设置强 secret
- 添加环境变量验证逻辑

**影响**: 完全的身份认证绕过

---

## High 风险 (1周内修复)

### 2. 默认管理员凭证弱且固定

**位置**: `src/db/mod.rs:36-53`

```rust
let password = crate::auth::password::hash("admin")?;
// ...
.bind("admin")
```

**风险**: 如果用户不修改默认管理员密码，任何知道默认凭证的人都可以获得 super_admin 权限。

**缓解方案**:
- 首次登录时强制修改密码
- 使用随机生成的初始密码并写入日志
- 添加密码复杂度要求

**影响**: 完全的系统访问

### 3. Prometheus Metrics 端点公开

**位置**: `src/api/router.rs:46-47`

```rust
.route("/metrics", get(handlers::metrics::prometheus_metrics))
```

**风险**: `/metrics` 端点没有认证保护，可能泄露敏感信息（系统指标、用户活动等）。

**缓解方案**:
- 添加 basic auth 或 token 保护
- 或限制仅内网访问
- 提供配置选项控制

**影响**: 信息泄露

### 4. 缺少 Rate Limiting

**位置**: 所有 API 端点

**风险**: 没有 API 速率限制，易受暴力破解、DDoS 攻击。

**缓解方案**:
- 使用 tower-limits 或类似中间件
- 对登录端点实施特别限制
- 考虑 IP 级限流

**影响**: DoS 攻击、暴力破解

---

## Medium 风险 (2-4周修复)

### 5. JWT 不支持撤销

**位置**: `src/auth/jwt.rs`

**风险**: JWT 一旦签发无法撤销，即使用户被禁用或删除，token 仍有效直到过期。

**缓解方案**:
- 实现 token 黑名单 (Redis 或数据库)
- 或缩短 JWT 有效期并使用 refresh token

**影响**: 权限提升风险

### 6. 缺少审计日志完整性保护

**位置**: `src/api/middleware/audit.rs`

**风险**: 审计日志可能被有权限的用户篡改。

**缓解方案**:
- 审计日志使用单独的数据库连接
- 添加哈希校验链
- 定期导出到不可变存储

**影响**: 审计完整性丧失

### 7. 缺少 CORS 配置审查

**位置**: `src/api/mod.rs`

**风险**: 未验证 CORS 配置是否正确限制来源。

**缓解方案**:
- 审查 CORS 中间件配置
- 确保生产环境限制 allowed origins

**影响**: CSRF 风险

### 8. 前端依赖可能存在已知漏洞

**位置**: `frontend/package.json`

**风险**: 前端依赖未进行漏洞扫描。

**缓解方案**:
- 添加 `npm audit` 到 CI/CD
- 使用 Dependabot 自动更新
- 定期运行 Snyk 扫描

**影响**: XSS、其他客户端攻击

---

## Low 风险 (后续优化)

### 9. 默认密码复杂度要求不足

**位置**: `src/api/handlers/users.rs:90-91`

```rust
if body.password.len() < 8 {
    return Err(AppError::Validation("Password must be at least 8 characters".to_string()));
}
```

**缓解**: 添加大小写、数字、特殊字符要求

### 10. 缺少账户锁定机制

**位置**: 登录流程

**缓解**: 多次失败后临时锁定账户或 IP

### 11. 日志可能包含敏感信息

**位置**: 各处 `tracing::debug!`

**缓解**: 审查日志输出，避免记录敏感数据

---

## 安全亮点 (做得好的地方)

| 项目 | 评价 |
|------|------|
| **密码哈希** | ✅ 使用 Argon2，行业标准 |
| **SQL 注入防护** | ✅ sqlx 参数化查询 |
| **RBAC 实现** | ✅ 清晰的角色和权限系统 |
| **Docker 安全** | ✅ 非 root 用户运行 |
| **代码安全** | ✅ 无 unsafe 代码 |
| **环境变量** | ✅ .env.example 正确，.env 不提交 |
| **JWT 实现** | ✅ 使用 jsonwebtoken 库 |
| **前端技术** | ✅ React + shadcn/ui，现代栈 |

---

## STRIDE 威胁建模

| 威胁类型 | 评估 | 主要风险 |
|----------|------|----------|
| **Spoofing** (伪造) | 高 | JWT secret 泄露、默认凭证 |
| **Tampering** (篡改) | 中 | 审计日志完整性 |
| **Repudiation** (抵赖) | 低 | 审计日志记录用户操作 |
| **Information Disclosure** (信息泄露) | 中 | /metrics 端点公开 |
| **Denial of Service** (拒绝服务) | 高 | 缺少 rate limiting |
| **Elevation of Privilege** (权限提升) | 高 | JWT 撤销问题 |

---

## Pre-Mortem 分析

假设 Ent-DNS 在生产环境中发生严重安全事件，可能的原因：

### 1. 默认配置未修改
- 用户直接运行，未修改 JWT secret
- 默认 admin/admin 凭证未更改
- **预防**: 启动时强制检查，拒绝默认值

### 2. JWT Secret 泄露
- 日志意外记录 secret
- 配置文件提交到公开仓库
- **预防**: 环境变量强制、.gitignore 检查

### 3. 暴力破解成功
- 没有 rate limiting
- 无账户锁定机制
- **预防**: 实施速率限制和账户锁定

### 4. 内部人员滥用
- 审计日志可被篡改
- 无操作二次确认
- **预防**: 审计日志保护、敏感操作确认

### 5. 前端供应链攻击
- 依赖包含恶意代码
- npm 包劫持
- **预防**: 锁定依赖版本、审计 CI/CD

---

## 修复优先级路线图

### 第1周 (立即行动)
- [ ] 强制 JWT secret 验证
- [ ] 默认管理员密码强制首次修改
- [ ] 添加 /metrics 认证
- [ ] 实施 rate limiting

### 第2-4周 (高优先级)
- [ ] JWT 撤销机制
- [ ] 审计日志保护
- [ ] CORS 配置审查
- [ ] 前端依赖漏洞扫描

### 第5-8周 (中优先级)
- [ ] 密码策略增强
- [ ] 账户锁定机制
- [ ] 日志安全审查
- [ ] 安全头配置 (CSP, HSTS 等)

---

## 推荐工具

| 用途 | 工具 |
|------|------|
| 依赖扫描 | `cargo audit`, `npm audit`, Snyk |
| 密钥检测 | gitleaks, truffleHog |
| 配置检查 | checkov, tfsec |
| 渗透测试 | OWASP ZAP, Burp Suite |
| 静态分析 | Clippy, ESLint security plugin |

---

## 结论

Ent-DNS 的安全基础扎实，使用了成熟的加密和安全库。**Critical 风险主要集中在默认配置问题上**，这是可以快速修复的。

建议在部署到生产环境前，**必须修复所有 Critical 和 High 风险**，并建立定期的安全审计流程。

---

**审计人**: critic-munger (Charlie Munger AI)
**审核日期**: 2026-02-19
**下次审计建议**: 3 个月后或重大版本更新时
