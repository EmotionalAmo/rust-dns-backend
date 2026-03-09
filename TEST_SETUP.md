# 测试环境设置指南

## 概述

本项目已经从 SQLite 迁移到 PostgreSQL。所有测试现在使用 PostgreSQL 数据库运行。

## 前置要求

### 1. 安装 PostgreSQL

#### macOS (使用 Homebrew)
```bash
brew install postgresql@16
brew services start postgresql@16
```

#### Ubuntu/Debian
```bash
sudo apt-get install postgresql postgresql-contrib
sudo systemctl start postgresql
```

#### Docker (推荐用于隔离测试)
```bash
docker run --name rust-dns-test-db \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_DB=rust_dns_test \
  -p 5432:5432 \
  -d postgres:16-alpine
```

### 2. 创建测试数据库

```sql
-- 连接到 PostgreSQL
psql -U postgres

-- 创建测试数据库
CREATE DATABASE rust_dns_test;

-- 退出
\q
```

### 3. 配置环境变量

设置 `TEST_DATABASE_URL` 环境变量（可选，有默认值）：

```bash
export TEST_DATABASE_URL="postgres://postgres:postgres@localhost:5432/rust_dns_test"
```

默认值：`postgres://postgres:postgres@localhost:5432/rust_dns_test`

## 运行测试

### 运行所有测试
```bash
cargo test
```

### 运行特定测试文件
```bash
cargo test --test api_integration
cargo test --test client_groups_e2e
cargo test --test alerts_sandbox_e2e
```

### 运行被忽略的测试（如性能测试）
```bash
cargo test -- --ignored
```

### 运行特定测试函数
```bash
cargo test test_login_success_returns_token
```

## 测试文件说明

| 测试文件 | 说明 | 数据库使用 |
|---------|------|-----------|
| `tests/api_integration.rs` | API 集成测试（认证、规则、查询日志等） | PostgreSQL |
| `tests/client_groups_e2e.rs` | 客户端组和规则 E2E 测试 | PostgreSQL |
| `tests/alerts_sandbox_e2e.rs` | 警报和沙盒 API 测试 | PostgreSQL |
| `tests/oisd_performance_test.rs` | OISD 规则集性能测试（需要网络） | PostgreSQL |
| `tests/graceful_shutdown.rs` | 优雅关闭功能测试 | 无需数据库 |

## 常见问题

### 1. 连接数据库失败
```
error: connection to server at "localhost", port 5432 failed
```

**解决方案**：
- 确保 PostgreSQL 服务正在运行
- 检查连接字符串是否正确
- 验证用户名和密码

### 2. 权限错误
```
error: permission denied for database rust_dns_test
```

**解决方案**：
```sql
GRANT ALL PRIVILEGES ON DATABASE rust_dns_test TO postgres;
GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA public TO postgres;
```

### 3. 迁移失败
```
error: Migration failed
```

**解决方案**：
- 删除并重新创建测试数据库
- 检查迁移文件是否正确

```sql
DROP DATABASE IF EXISTS rust_dns_test;
CREATE DATABASE rust_dns_test;
```

### 4. Docker 容器问题
```
error: could not connect to server
```

**解决方案**：
```bash
# 检查容器状态
docker ps | grep rust-dns-test-db

# 查看容器日志
docker logs rust-dns-test-db

# 重启容器
docker restart rust-dns-test-db
```

## CI/CD 集成

在 GitHub Actions 中使用 PostgreSQL 服务：

```yaml
services:
  postgres:
    image: postgres:16-alpine
    env:
      POSTGRES_PASSWORD: postgres
      POSTGRES_USER: postgres
      POSTGRES_DB: rust_dns_test
    options: >-
      --health-cmd pg_isready
      --health-interval 10s
      --health-timeout 5s
      --health-retries 5

env:
  TEST_DATABASE_URL: postgres://postgres:postgres@localhost:5432/rust_dns_test
```

## 性能测试注意事项

`tests/oisd_performance_test.rs` 默认被忽略（`#[ignore]`），因为：
- 需要网络连接下载 OISD 规则集
- 测试时间较长（约 1-2 分钟）
- 测试 50,000 条规则的加载和查询性能

运行性能测试：
```bash
cargo test -- --ignored test_oisd_performance
```

## 数据库清理

测试后清理数据库：
```bash
# 方式 1：删除并重新创建数据库
psql -U postgres -c "DROP DATABASE IF EXISTS rust_dns_test; CREATE DATABASE rust_dns_test;"

# 方式 2：清空所有表（保留数据库结构）
psql -U postgres rust_dns_test -c "\dt" | grep -o "^\S\+" | xargs -I {} psql -U postgres rust_dns_test -c "TRUNCATE TABLE {} CASCADE;"
```

## 下一步

- 确保所有测试通过
- 检查测试覆盖率
- 添加更多集成测试
- 优化测试性能
