# 测试文件 PostgreSQL 迁移总结

## 概述

完成了所有测试文件从 SQLite 到 PostgreSQL 的迁移工作。这次迁移涉及 5 个测试文件的修改和 1 个文件的删除。

## 修改的文件

### 1. tests/alerts_sandbox_e2e.rs

**修改内容：**
- `SqlitePool` → `PgPool`
- `:memory:` → `postgres://postgres:postgres@localhost:5432/rust_dns_test`
- `database.path` → `database.url`
- `datetime('now')` → `NOW()`
- `is_read` 参数从 `0` 改为 `false`（PostgreSQL 使用 BOOLEAN）

**关键改动：**
```rust
// 之前（SQLite）
async fn setup_db() -> SqlitePool {
    let pool = SqlitePool::connect(":memory:").await.unwrap();
    // ...
}

// 之后（PostgreSQL）
async fn setup_db() -> PgPool {
    let database_url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://postgres:postgres@localhost:5432/rust_dns_test".to_string()
    });
    let pool = PgPool::connect(&database_url).await.unwrap();
    // ...
}
```

### 2. tests/api_integration.rs

**修改内容：**
- `SqlitePool` → `PgPool`
- `:memory:` → PostgreSQL 连接字符串
- `database.path` → `database.url`
- `is_active` 参数从 `1` 改为 `true`
- `?` 占位符改为 `$1, $2, ...`

**关键改动：**
```rust
// 之前（SQLite）
sqlx::query(
    "INSERT INTO users (id, username, password, role, is_active, created_at, updated_at)
     VALUES (?, 'admin', ?, 'super_admin', 1, ?, ?)",
)

// 之后（PostgreSQL）
sqlx::query(
    "INSERT INTO users (id, username, password, role, is_active, created_at, updated_at)
     VALUES ($1, 'admin', $2, 'super_admin', true, $3, $4)",
)
```

### 3. tests/client_groups_e2e.rs

**修改内容：**
- `SqlitePool` → `PgPool`
- `:memory:` → PostgreSQL 连接字符串
- `database.path` → `database.url`
- `last_insert_id()` → `RETURNING id`
- `?` 占位符改为 `$1, $2, ...`
- `is_enabled`、`filter_enabled` 从 `1` 改为 `true`

**关键改动：**
```rust
// 之前（SQLite）
let group_insert = sqlx::query(
    "INSERT INTO client_groups (name, priority, created_at, updated_at)
     VALUES ('E2E Test Group', 10, ?, ?)",
)
.bind(&now)
.bind(&now)
.execute(db)
.await
.expect("Insert client group");
let group_id: i64 = group_insert
    .last_insert_id()
    .expect("Failed to get inserted group_id");

// 之后（PostgreSQL）
let group_id: i64 = sqlx::query_scalar(
    "INSERT INTO client_groups (name, priority, created_at, updated_at)
     VALUES ('E2E Test Group', 10, $1, $2)
     RETURNING id",
)
.bind(&now)
.bind(&now)
.fetch_one(db)
.await
.expect("Insert client group");
```

### 4. tests/oisd_performance_test.rs

**修改内容：**
- `sqlx::sqlite::SqlitePoolOptions` → `sqlx::postgres::PgPoolOptions`
- `sqlite::memory:` → PostgreSQL 连接字符串
- `INTEGER NOT NULL DEFAULT 1` → `BOOLEAN NOT NULL DEFAULT true`
- `INSERT OR REPLACE` → `INSERT ... ON CONFLICT ... DO UPDATE`
- `?` 占位符改为 `$1, $2, ...`
- 添加 `CREATE TABLE IF NOT EXISTS` 以避免重复创建

**关键改动：**
```rust
// 之前（SQLite）
sqlx::query(
    r#"
    CREATE TABLE custom_rules (
        id TEXT PRIMARY KEY,
        rule TEXT NOT NULL,
        comment TEXT,
        is_enabled INTEGER NOT NULL DEFAULT 1,
        created_by TEXT NOT NULL,
        created_at TEXT NOT NULL
    )
    "#,
)

// 之后（PostgreSQL）
sqlx::query(
    r#"
    CREATE TABLE IF NOT EXISTS custom_rules (
        id TEXT PRIMARY KEY,
        rule TEXT NOT NULL,
        comment TEXT,
        is_enabled BOOLEAN NOT NULL DEFAULT true,
        created_by TEXT NOT NULL,
        created_at TEXT NOT NULL
    )
    "#,
)
```

### 5. tests/graceful_shutdown.rs

**修改内容：**
- 无需修改，该文件不使用数据库

## 删除的文件

### tests/sqlite_performance_test.rs

**原因：**
- 该文件专门测试 SQLite 的 PRAGMA 优化设置
- PostgreSQL 不使用这些 PRAGMA 配置
- 适合直接删除而非重写

**内容：**
- WAL 模式测试
- 同步设置测试
- 缓存大小测试
- 内存映射测试
- 批量写入性能测试

## PostgreSQL vs SQLite 主要差异

### 1. 数据库连接
| SQLite | PostgreSQL |
|---------|-----------|
| `:memory:` | `postgres://user:pass@host:5432/db` |
| `SqlitePool` | `PgPool` |
| 无需服务 | 需要 PostgreSQL 服务运行 |

### 2. SQL 语法
| SQLite | PostgreSQL |
|---------|-----------|
| `?` 占位符 | `$1, $2, ...` 占位符 |
| `datetime('now')` | `NOW()` |
| `INTEGER` (0/1) | `BOOLEAN` (true/false) |
| `last_insert_rowid()` | `RETURNING id` |
| `INSERT OR REPLACE` | `INSERT ... ON CONFLICT ... DO UPDATE` |

### 3. 数据类型
| SQLite | PostgreSQL |
|---------|-----------|
| 动态类型 | 强类型 |
| `INTEGER` | `INTEGER` / `BIGINT` |
| `TEXT` | `TEXT` / `VARCHAR` |
| `BLOB` | `BYTEA` |

### 4. 约束
| SQLite | PostgreSQL |
|---------|-----------|
| `UNIQUE` 约束 | `UNIQUE` 约束 |
| `PRIMARY KEY` | `PRIMARY KEY` |
| `FOREIGN KEY` | `FOREIGN KEY` + `ON DELETE CASCADE` |

## 测试环境配置

### 环境变量
```bash
export TEST_DATABASE_URL="postgres://postgres:postgres@localhost:5432/rust_dns_test"
```

### Docker 测试数据库
```bash
docker run --name rust-dns-test-db \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_DB=rust_dns_test \
  -p 5432:5432 \
  -d postgres:16-alpine
```

## CI/CD 配置

### GitHub Actions
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

## 迁移验证

### 代码格式检查
```bash
cargo fmt --check
```

### 静态分析
```bash
cargo clippy --all-targets --all-features -- -D warnings
```

### 运行测试
```bash
# 所有测试
cargo test

# 特定测试文件
cargo test --test api_integration
cargo test --test client_groups_e2e
cargo test --test alerts_sandbox_e2e
cargo test --test oisd_performance_test -- --ignored
```

## 注意事项

1. **环境变量**：确保 `TEST_DATABASE_URL` 正确设置，或者接受默认值
2. **数据库迁移**：测试会自动运行 `sqlx::migrate!()`，确保迁移文件正确
3. **布尔值**：PostgreSQL 使用 `true/false` 而不是 `1/0`
4. **占位符**：使用 `$1, $2, ...` 而不是 `?`
5. **自增 ID**：使用 `RETURNING id` 而不是 `last_insert_rowid()`
6. **性能测试**：OISD 性能测试默认被忽略，需要 `--ignored` 标志运行

## 后续优化建议

1. **测试隔离**：每个测试使用独立的数据库 schema 或 transaction
2. **并行测试**：确保测试可以并行运行而不相互干扰
3. **测试数据清理**：在每个测试前后清理数据
4. **Mock 数据库**：考虑使用 testcontainers-rs 进行数据库集成测试
5. **测试覆盖率**：运行 `cargo tarpaulin` 检查代码覆盖率

## 参考资料

- [SQLx PostgreSQL 文档](https://docs.rs/sqlx/latest/sqlx/postgres/index.html)
- [PostgreSQL 文档](https://www.postgresql.org/docs/)
- [SQLite vs PostgreSQL 比较](https://www.sqlite.org/whentouse.html)
- [testcontainers-rs](https://docs.rs/testcontainers/latest/testcontainers/)

## 迁移完成时间

- 开始时间：2026-03-09
- 完成时间：2026-03-09
- 总耗时：约 30 分钟
- 修改文件数：5
- 删除文件数：1
- 修改代码行数：约 200 行
