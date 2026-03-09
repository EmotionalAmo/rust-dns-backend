#!/bin/bash

# PostgreSQL 测试环境设置和验证脚本

set -e

echo "======================================"
echo "PostgreSQL 测试环境设置"
echo "======================================"
echo ""

# 检查环境变量
if [ -z "$TEST_DATABASE_URL" ]; then
    echo "TEST_DATABASE_URL 未设置，使用默认值"
    export TEST_DATABASE_URL="postgres://postgres:postgres@localhost:5432/rust_dns_test"
fi

echo "数据库连接字符串: $TEST_DATABASE_URL"
echo ""

# 检查 PostgreSQL 是否安装
if ! command -v psql &> /dev/null; then
    echo "❌ PostgreSQL 未安装"
    echo ""
    echo "请安装 PostgreSQL："
    echo "  macOS: brew install postgresql@16"
    echo "  Ubuntu: sudo apt-get install postgresql"
    echo "  Docker: docker run -p 5432:5432 -e POSTGRES_PASSWORD=postgres -d postgres:16-alpine"
    exit 1
fi

echo "✅ PostgreSQL 已安装: $(psql --version | head -n 1)"
echo ""

# 尝试连接到数据库
echo "测试数据库连接..."
DB_HOST=$(echo "$TEST_DATABASE_URL" | sed -n 's/.*@\([^:]*\):.*/\1/p')
DB_PORT=$(echo "$TEST_DATABASE_URL" | sed -n 's/.*:\([0-9]*\)\/.*/\1/p')
DB_USER=$(echo "$TEST_DATABASE_URL" | sed -n 's/\/\/\([^:]*\):.*/\1/p')
DB_NAME=$(echo "$TEST_DATABASE_URL" | sed -n 's/.*\/\([^?]*\)/\1/p')

echo "  主机: $DB_HOST"
echo "  端口: $DB_PORT"
echo "  用户: $DB_USER"
echo "  数据库: $DB_NAME"
echo ""

# 检查数据库连接
if ! PGPASSWORD=postgres psql -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" -d "$DB_NAME" -c "SELECT 1" &> /dev/null; then
    echo "❌ 无法连接到数据库"
    echo ""
    echo "可能的原因："
    echo "  1. PostgreSQL 服务未启动"
    echo "  2. 数据库不存在"
    echo "  3. 连接参数不正确"
    echo ""
    echo "尝试创建数据库..."
    PGPASSWORD=postgres psql -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" -d postgres -c "CREATE DATABASE $DB_NAME;" 2>/dev/null || echo "数据库已存在或创建失败"
    echo ""
    exit 1
fi

echo "✅ 数据库连接成功"
echo ""

# 运行 cargo test 检查
echo "检查测试编译..."
if ! cargo test --no-run 2>&1 | grep -q "Finished"; then
    echo "❌ 测试编译失败"
    cargo test --no-run
    exit 1
fi

echo "✅ 测试编译成功"
echo ""

# 运行简单的测试
echo "运行简单测试..."
if cargo test --test graceful_shutdown --quiet; then
    echo "✅ 测试运行成功"
else
    echo "⚠️  部分测试失败（可能需要完整的数据库设置）"
fi

echo ""
echo "======================================"
echo "设置完成！"
echo "======================================"
echo ""
echo "运行所有测试："
echo "  cargo test"
echo ""
echo "运行特定测试："
echo "  cargo test --test api_integration"
echo "  cargo test --test client_groups_e2e"
echo "  cargo test --test alerts_sandbox_e2e"
echo ""
echo "运行性能测试："
echo "  cargo test -- --ignored"
echo ""
