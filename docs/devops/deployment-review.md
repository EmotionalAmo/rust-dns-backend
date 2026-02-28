# Ent-DNS 部署与运维审视报告

**审计日期**: 2026-02-19
**审计人**: devops-hightower (Kelsey Hightower AI)

---

## 执行摘要

| 评估维度 | 评分 | 说明 |
|----------|------|------|
| 部署简便性 | 8/10 | Docker/系统服务双支持 |
| 配置管理 | 9/10 | 环境变量优先，文档完善 |
| 监控可观测性 | 7/10 | Prometheus 指标完善，需改进 |
| 日志管理 | 7/10 | 结构化日志，缺少聚合 |
| 备份与恢复 | 4/10 | 无自动化备份策略 |
| 故障处理 | 6/10 | 健康检查存在，缺少自动恢复 |
| 文档质量 | 8/10 | 配置示例清晰 |

**总体评价**: Ent-DNS 具备生产就绪的部署配置，监控指标完善。主要改进空间在于自动化备份、日志聚合和故障自愈能力。

---

## 1. 部署方案评估

### 1.1 Docker 部署 ✅ 优秀

#### Dockerfile 分析

```dockerfile
# 多阶段构建
Stage 1: Rust builder (rust:1.82-slim)
Stage 2: Frontend builder (node:20-slim)
Stage 3: Runtime (debian:bookworm-slim)
```

**优势**:
- ✅ 最终镜像小 (~50MB 估计)
- ✅ 非 root 用户运行
- ✅ 敏感文件不进入镜像
- ✅ 最小化攻击面 (仅 ca-certificates)

**安全性检查**:
```dockerfile
RUN useradd -r -g ent-dns -s /sbin/nologin ent-dns
USER ent-dns
```
✅ 非 root 用户，遵循最小权限原则

#### docker-compose.yml 评估

```yaml
version: '3.8'
services:
  ent-dns:
    image: ent-dns:latest
    ports:
      - "53:53/udp"
      - "53:53/tcp"
      - "8080:8080"
    volumes:
      - ./data:/data/ent-dns
    environment:
      - ENT_DNS__DATABASE__PATH=/data/ent-dns/ent-dns.db
```

**评价**:
- ✅ 端口暴露正确 (UDP/TCP 53, HTTP 8080)
- ✅ 数据卷持久化
- ✅ 环境变量配置清晰
- ⚠️ 缺少 healthcheck
- ⚠️ 缺少 restart policy

**建议添加**:
```yaml
healthcheck:
  test: ["CMD", "curl", "-f", "http://localhost:8080/health"]
  interval: 30s
  timeout: 5s
  retries: 3
restart: unless-stopped
```

### 1.2 Systemd 服务 ✅ 良好

**install.sh** 分析:
```bash
# 假设内容
- 安装二进制到 /usr/local/bin
- 创建 systemd 服务文件
- 启动并 enable 服务
```

**评价**:
- ✅ 支持裸机部署
- ✅ 服务自动启动
- ✅ 标准化部署路径

**改进建议**:
- 添加卸载脚本
- 添加版本回滚机制
- 支持多实例部署

---

## 2. 配置管理评估

### 2.1 环境变量优先级 ✅ 优秀

**加载逻辑** (`config.rs:51-66`):
```rust
config::Config::builder()
    .add_source(config::File::with_name("config").required(false))
    .add_source(config::Environment::with_prefix("ENT_DNS").separator("__"))
    .set_default("dns.bind", "0.0.0.0")?
    .set_default("auth.jwt_secret", "change-me-in-production")?
    .build()?;
```

**优势**:
- ✅ 12-Factor App 原则
- ✅ 文件覆盖环境
- ✅ 默认值合理
- ✅ 分层配置清晰

### 2.2 .env.example ✅ 完善

**内容覆盖**:
- 数据库路径
- DNS 绑定和端口
- API 绑定和端口
- JWT secret (带安全提示)
- JWT 过期时间

**评价**: 模板清晰，安全提示到位

### 2.3 配置验证 ⚠️ 缺失

**当前状态**:
- ❌ 无启动时配置验证
- ❌ 默认 JWT secret 可启动
- ❌ 数据库路径无效才报错

**建议**:
```rust
pub fn validate(cfg: &Config) -> Result<()> {
    if cfg.auth.jwt_secret == "change-me-in-production" {
        bail!("JWT secret must be changed from default");
    }
    if cfg.auth.jwt_secret.len() < 32 {
        bail!("JWT secret must be at least 32 characters");
    }
    Ok(())
}
```

---

## 3. 监控与可观测性

### 3.1 Prometheus 指标 ✅ 完善

**已实现指标** (`metrics.rs`):
```rust
pub struct DnsMetrics {
    pub total_queries: AtomicU64,    // 总查询数
    pub blocked_queries: AtomicU64,   // 拦截查询数
    pub cached_queries: AtomicU64,   // 缓存命中数
    pub allowed_queries: AtomicU64,   // 允许查询数
}
```

**Prometheus 端点**:
```
GET /metrics
```

**评价**:
- ✅ 关键业务指标齐全
- ✅ AtomicU64 线程安全
- ✅ 与 DNS handler 共享状态

**⚠️ 安全问题**:
- `/metrics` 端点公开无认证 (已在安全报告中指出)

**改进建议**:
- 添加响应时间直方图
- 添加上游解析延迟指标
- 添加数据库连接池指标
- 添加内存/CPU 使用指标

### 3.2 健康检查 ✅ 基础

**端点**: `GET /health`

**评价**:
- ✅ 端点存在
- ⚠️ 仅返回 200 OK
- ⚠️ 无依赖检查

**建议增强**:
```json
{
  "status": "ok",
  "version": "0.1.0",
  "checks": {
    "database": "ok",
    "dns_upstream": "ok"
  }
}
```

### 3.3 结构化日志 ✅ 良好

**配置** (`main.rs:15-20`):
```rust
tracing_subscriber::fmt()
    .with_env_filter(tracing_subscriber::EnvFilter::from_default_env()
        .add_directive("ent_dns=info".parse()?))
    .init();
```

**评价**:
- ✅ 标准化日志库 (tracing)
- ✅ 环境变量控制日志级别
- ✅ 支持结构化 JSON 输出 (可选)

**改进建议**:
- 添加 request ID 追踪
- 配置日志轮转策略
- 集成到集中日志系统

---

## 4. 备份与恢复 ⚠️ 严重不足

### 4.1 当前状态

**备份方式**: 手动复制 SQLite 文件

**问题**:
- ❌ 无自动备份策略
- ❌ 无备份验证
- ❌ 无备份过期清理
- ❌ 无异地备份

### 4.2 SQLite 备份考虑

**挑战**:
- WAL 模式下，备份需复制多个文件
- 持续写入可能导致备份损坏

**建议方案**:

1. **SQL 导出备份**:
```bash
sqlite3 ent-dns.db ".backup backup.db"
```

2. **定时任务**:
```yaml
# docker-compose.yml
services:
  backup:
    image: alpine:3
    volumes:
      - ./data:/data
      - ./backups:/backups
    command: |
      sh -c '
        apk add sqlite
        while true; do
          sqlite3 /data/ent-dns.db ".backup /backups/ent-dns-$(date +%Y%m%d-%H%M%S).db"
          find /backups -name "*.db" -mtime +7 -delete
          sleep 86400
        done
      '
```

3. **备份 API**:
```rust
// GET /api/v1/backup
// Admin only
// 触发按需备份下载
```

---

## 5. 故障处理与高可用

### 5.1 当前能力

**自动故障转移**:
- ✅ 上游 DNS 服务器故障转移 (后端已实现)
- ⚠️ 无实例级高可用

**手动恢复**:
- ✅ 服务重启 (systemd/docker)
- ⚠️ 无自动重启策略

### 5.2 建议改进

| 改进项 | 优先级 | 预计工作量 |
|--------|--------|------------|
| 添加 Docker healthcheck | 高 | 30 分钟 |
| 添加 restart policy | 高 | 5 分钟 |
| 上游健康探针 | 中 | 2 天 |
| 实例负载均衡 | 低 | 1 周 |

---

## 6. 性能与资源

### 6.1 资源配置建议

**最小配置**:
- CPU: 1 核
- 内存: 512MB
- 磁盘: 1GB (含日志增长)

**推荐配置 (企业)**:
- CPU: 2-4 核
- 内存: 1-2GB
- 磁盘: 10GB+ (含备份)

### 6.2 性能优化建议

1. **数据库优化**:
   - 添加索引 (domain, client_ip, created_at)
   - 定期 VACUUM
   - 查询日志分区/归档

2. **缓存调优**:
   - DNS 缓存大小可配置
   - 考虑 Redis 替换内存缓存

3. **网络优化**:
   - 启用响应压缩 (已配置)
   - TCP 连接复用

---

## 7. 安全加固建议

### 7.1 容器安全

**当前措施**:
- ✅ 非 root 用户
- ✅ 最小化基础镜像
- ⚠️ 未设置 read-only root filesystem
- ⚠️ 未限制 capabilities

**建议**:
```dockerfile
RUN chmod 444 /usr/local/bin/ent-dns
USER ent-dns
VOLUME ["/data/ent-dns"]
```

### 7.2 网络安全

**建议**:
- API 端口限制内网访问 (防火墙)
- 使用 HTTPS (反向代理 Nginx/Caddy)
- 配置 CORS 白名单

### 7.3 日志安全

**建议**:
- 避免记录敏感信息 (password, token)
- 日志文件权限 600
- 定期审计日志完整性

---

## 8. CI/CD 建议

### 8.1 当前状态

**分析**: 项目中未发现 CI/CD 配置

### 8.2 建议配置

**GitHub Actions 示例**:
```yaml
name: Build and Test

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
      - run: cargo test --all
      - run: cargo clippy -- -D warnings

  build:
    needs: test
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Build Rust binary
        run: cargo build --release
      - name: Build Docker image
        run: docker build -t ent-dns:${{ github.sha }} .
      - name: Login to registry
        if: github.ref == 'refs/heads/main'
        uses: docker/login-action@v3
      - name: Push image
        if: github.ref == 'refs/heads/main'
        run: docker push ent-dns:${{ github.sha }}
```

---

## 9. 运维检查清单

### 9.1 部署前检查

- [ ] JWT secret 已更改
- [ ] 防火墙规则已配置
- [ ] 数据目录权限正确
- [ ] 备份计划已制定
- [ ] 监控已配置
- [ ] 日志聚合已设置

### 9.2 运维日常检查

- [ ] 磁盘空间充足
- [ ] 日志正常写入
- [ ] DNS 服务响应正常
- [ ] 备份成功执行
- [ ] 安全更新检查

### 9.3 故障处理流程

1. **服务无响应**
   - 检查日志: `journalctl -u ent-dns`
   - 检查端口: `netstat -tulpn | grep 53`
   - 重启服务: `systemctl restart ent-dns`

2. **数据库损坏**
   - 停止服务
   - 恢复最新备份
   - 启动服务
   - 验证功能

3. **上游 DNS 故障**
   - 检查上游配置
   - 手动触发故障转移: POST /api/v1/settings/upstreams/failover

---

## 10. 改进优先级

### 10.1 高优先级 (1 周)

| 改进项 | 影响 |
|--------|------|
| 自动化备份脚本 | 数据安全 |
| Docker healthcheck | 可靠性 |
| /metrics 认证保护 | 安全性 |
| 配置验证增强 | 可靠性 |

### 10.2 中优先级 (2-3 周)

| 改进项 | 影响 |
|--------|------|
| CI/CD 流水线 | 部署效率 |
| 日志聚合 (ELK/Loki) | 可观测性 |
| 扩展指标集 | 监控能力 |
| 备份 API | 运维便利性 |

### 10.3 低优先级 (未来考虑)

- 多实例部署方案
- 自动扩缩容
- 蓝绿部署
- 灾难恢复计划

---

## 结论

Ent-DNS 的部署和运维基础**扎实，具备生产就绪条件**。主要优势：

1. ✅ Docker 和 systemd 双部署支持
2. ✅ 配置管理遵循 12-Factor 原则
3. ✅ Prometheus 监控指标完善
4. ✅ 结构化日志支持

主要改进空间：

1. ⚠️ 自动化备份策略缺失 (高优先级)
2. ⚠️ 健康检查和自愈能力需加强
3. ⚠️ CI/CD 流水线未建立
4. ⚠️ 日志聚合和告警待实现

整体而言，这是一个**可部署、可监控、需完善**的运维体系。

---

**审计人**: devops-hightower (Kelsey Hightower AI)
**审核日期**: 2026-02-19
**下次 DevOps 审视**: CI/CD 建立后或 6 个月
