#!/bin/bash

# DNS 性能对比基准测试运行脚本

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# 颜色定义
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${BLUE}============================================${NC}"
echo -e "${BLUE}   DNS 性能对比基准测试${NC}"
echo -e "${BLUE}============================================${NC}"
echo ""

# 参数
CONCURRENCY=${1:-100}
DURATION=${2:-30}

echo -e "${YELLOW}配置:${NC}"
echo -e "  并发级别: ${CONCURRENCY}"
echo -e "  测试时长: ${DURATION} 秒"
echo ""

# 检查 rust-dns 是否运行
if ! lsof -Pi :5354 -sTCP:LISTEN -t >/dev/null 2>&1; then
    echo -e "${YELLOW}rust-dns 服务器未运行，正在启动...${NC}"
    cd "$PROJECT_DIR"
    cargo build --release >/dev/null 2>&1
    nohup ./target/release/rust-dns-server > /dev/null 2>&1 &
    RUST_PID=$!
    echo "  rust-dns PID: $RUST_PID"
    echo ""

    # 等待服务器启动
    echo -e "${YELLOW}等待服务器启动...${NC}"
    sleep 3

    # 检查是否成功启动
    if ! lsof -Pi :5354 -sTCP:LISTEN -t >/dev/null 2>&1; then
        echo -e "\033[0;31m错误: 无法启动 rust-dns 服务器${NC}"
        exit 1
    fi
else
    echo -e "${GREEN}rust-dns 已运行在 127.0.0.1:5354${NC}"
    RUST_PID=""
    echo ""
fi

# 编译对比工具
echo -e "${YELLOW}编译对比工具...${NC}"
cd "$SCRIPT_DIR"
cargo build --release --bin compare-dns 2>&1 | grep -E "Compiling|Finished|error|warning" || true
echo ""

# 运行对比测试
echo -e "${GREEN}开始测试...${NC}"
echo ""
./target/release/compare-dns "$CONCURRENCY" "$DURATION"
echo ""

# 清理
if [ -n "$RUST_PID" ]; then
    echo -e "${YELLOW}停止 rust-dns 服务器...${NC}"
    kill "$RUST_PID" 2>/dev/null || true
fi

echo -e "${GREEN}============================================${NC}"
echo -e "${GREEN}   测试完成!${NC}"
echo -e "${GREEN}============================================${NC}"
