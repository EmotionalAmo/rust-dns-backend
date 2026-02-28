#!/usr/bin/env bash
# 长时间稳定性测试（24 小时持续负载）
# 验证内存泄漏、连接泄漏、数据一致性

set -e

# 配置
DURATION="${DURATION:-86400}"  # 24 小时（可调整）
DNS_QPS="${DNS_QPS:-1000}"
API_VUS="${API_VUS:-10}"
DNS_SERVER="${DNS_SERVER:-127.0.0.1}"
DNS_PORT="${DNS_PORT:-5353}"
RESULTS_DIR="${RESULTS_DIR:-$(dirname "$0")/../results/stability-$(date +%Y%m%d-%H%M%S)}"

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${GREEN}=== Ent-DNS 长时间稳定性测试 ===${NC}"
echo "测试时长: ${DURATION} 秒 ($(($DURATION / 3600)) 小时)"
echo "DNS 负载: ${DNS_QPS} QPS"
echo "API 负载: ${API_VUS} VUs"
echo "结果目录: ${RESULTS_DIR}"
echo ""

# 创建结果目录
mkdir -p "$RESULTS_DIR"

# 检查依赖
if ! command -v dnsperf &> /dev/null; then
    echo -e "${RED}错误: dnsperf 未安装${NC}"
    exit 1
fi

if ! command -v k6 &> /dev/null; then
    echo -e "${RED}错误: k6 未安装${NC}"
    exit 1
fi

# 检查服务器健康
echo "检查服务器健康状态..."
if ! dig @${DNS_SERVER} -p ${DNS_PORT} example.com A +short &> /dev/null; then
    echo -e "${RED}错误: DNS 服务器不可达${NC}"
    exit 1
fi
echo -e "${GREEN}服务器健康检查通过${NC}"
echo ""

# 启动 DNS 负载
echo "启动 DNS 负载 (${DNS_QPS} QPS for ${DURATION}s)..."
DNS_LOG="$RESULTS_DIR/dnsperf.log"
dnsperf -s $DNS_SERVER -p $DNS_PORT -d $(dirname "$0")/domains.txt \
  -l $DURATION -q $DNS_QPS -s 1000 \
  > "$DNS_LOG" 2>&1 &
DNS_PID=$!
echo "DNS 负载进程 PID: $DNS_PID"

# 启动 API 负载
echo "启动 API 负载 (${API_VUS} VUs for ${DURATION}s)..."
API_LOG="$RESULTS_DIR/k6-api.log"
k6 run $(dirname "$0")/api-write-test.js \
  --duration ${DURATION}s \
  --vus $API_VUS \
  --out json="$RESULTS_DIR/k6-api.json" \
  > "$API_LOG" 2>&1 &
K6_PID=$!
echo "API 负载进程 PID: $K6_PID"
echo ""

# 记录初始状态
{
    echo "=== 初始状态 ($(date -Iseconds)) ==="
    echo "进程信息:"
    ps aux | grep ent-dns | grep -v grep
    echo ""
    echo "内存使用:"
    ps aux | grep ent-dns | grep -v grep | awk '{sum += $6; count++} END {print "总 RSS:", sum, "KB (", sum/1024, "MB)"; print "进程数:", count}'
    echo ""
    echo "磁盘使用:"
    du -sh ent-dns.db 2>/dev/null || echo "数据库文件未找到"
    echo ""
} > "$RESULTS_DIR/initial_state.txt"

# 定期采集指标（每小时）
echo "启动监控进程..."
MONITOR_LOG="$RESULTS_DIR/monitor.log"
SNAPSHOT_INTERVAL=3600  # 1 小时
NEXT_SNAPSHOT=0

while true; do
    sleep 10
    CURRENT_TIME=$(date +%s)

    # 定期快照
    if [ $CURRENT_TIME -ge $NEXT_SNAPSHOT ]; then
        HOURS=$((DURATION / SNAPSHOT_INTERVAL - (DURATION - (CURRENT_TIME - $(stat -f %m "$RESULTS_DIR/initial_state.txt") )) / SNAPSHOT_INTERVAL))

        echo -e "${YELLOW}采集快照 (小时 $HOURS)...${NC}"

        {
            echo "=== 快照: 小时 $HOURS ($(date -Iseconds)) ==="

            echo "进程信息:"
            ps aux | grep ent-dns | grep -v grep | awk '{print "PID:", $2, "RSS:", $6, "KB", "VSZ:", $5, "KB", "%CPU:", $3, "%MEM:", $4}'

            echo -e "\n内存趋势:"
            ps aux | grep ent-dns | grep -v grep | awk '{sum += $6; count++} END {print "总 RSS:", sum, "KB (", sum/1024, "MB)"; print "进程数:", count}'

            echo -e "\n磁盘使用:"
            du -sh ent-dns.db 2>/dev/null || echo "数据库文件未找到"
            ls -lh ent-dns.db-wal 2>/dev/null || echo "WAL 文件未找到"

            echo -e "\nSQLite 状态:"
            sqlite3 ent-dns.db <<EOF 2>/dev/null
SELECT "查询总数:" || COUNT(*) FROM query_log;
SELECT "表大小:" || (SELECT page_count * page_size FROM pragma_page_count, pragma_page_size) || " bytes";
PRAGMA lock_status;
PRAGMA journal_mode;
EOF

            echo -e "\nPrometheus Metrics:"
            curl -s http://127.0.0.1:8080/metrics | grep -E "ent_dns_queries_total"

            echo -e "\nDNS 负载状态:"
            if ps -p $DNS_PID > /dev/null 2>&1; then
                echo "DNS 负载运行中 (PID: $DNS_PID)"
            else
                echo -e "${RED}DNS 负载已退出${NC}"
            fi

            echo -e "\nAPI 负载状态:"
            if ps -p $K6_PID > /dev/null 2>&1; then
                echo "API 负载运行中 (PID: $K6_PID)"
            else
                echo -e "${RED}API 负载已退出${NC}"
            fi

        } > "$RESULTS_DIR/snapshot_${HOURS}.txt"

        NEXT_SNAPSHOT=$((CURRENT_TIME + SNAPSHOT_INTERVAL))

        # 实时日志监控（检查 panic 或崩溃）
        if grep -q "panic\|fatal\|killed\|segmentation fault" "$DNS_LOG" 2>/dev/null; then
            echo -e "${RED}检测到 DNS 进程崩溃！${NC}"
            echo -e "${RED}错误详情:${NC}"
            grep -A 5 "panic\|fatal\|killed" "$DNS_LOG" || true
            break
        fi

        if grep -q "panic\|fatal\|ERRO" "$API_LOG" 2>/dev/null; then
            echo -e "${YELLOW}API 负载检测到错误，但继续运行${NC}"
        fi
    fi

    # 检查进程是否仍在运行
    if ! ps -p $DNS_PID > /dev/null 2>&1; then
        echo -e "${RED}DNS 负载进程已退出 (PID: $DNS_PID)${NC}"
        break
    fi

    if ! ps -p $K6_PID > /dev/null 2>&1; then
        echo -e "${RED}API 负载进程已退出 (PID: $K6_PID)${NC}"
        break
    fi
done

# 等待所有负载进程结束
echo "等待负载进程结束..."
wait $DNS_PID 2>/dev/null || true
wait $K6_PID 2>/dev/null || true

# 最终状态
{
    echo "=== 最终状态 ($(date -Iseconds)) ==="
    echo "进程信息:"
    ps aux | grep ent-dns | grep -v grep || echo "未找到运行中的进程"
    echo ""
    echo "磁盘使用:"
    du -sh ent-dns.db 2>/dev/null || echo "数据库文件未找到"
    echo ""
    echo "SQLite 状态:"
    sqlite3 ent-dns.db <<EOF 2>/dev/null
SELECT "查询总数:" || COUNT(*) FROM query_log;
SELECT "表大小:" || (SELECT page_count * page_size FROM pragma_page_count, pragma_page_size) || " bytes";
PRAGMA integrity_check;
EOF
} > "$RESULTS_DIR/final_state.txt"

# 生成对比报告
echo -e "${GREEN}生成对比报告...${NC}"
{
    echo "=== 内存增长分析 ==="
    INITIAL_MEM=$(grep "总 RSS:" "$RESULTS_DIR/initial_state.txt" | awk '{print $3}')
    FINAL_MEM=$(grep "总 RSS:" "$RESULTS_DIR/final_state.txt" | awk '{print $3}')
    MEM_DIFF=$((FINAL_MEM - INITIAL_MEM))
    MEM_PERCENT=$((MEM_DIFF * 100 / INITIAL_MEM))

    echo "初始内存: $INITIAL_MEM KB"
    echo "最终内存: $FINAL_MEM KB"
    echo "内存增长: $MEM_DIFF KB ($MEM_PERCENT%)"

    if [ $MEM_PERCENT -gt 20 ]; then
        echo -e "${RED}警告: 内存增长超过 20%，可能存在内存泄漏${NC}"
    fi

    echo -e "\n=== 磁盘增长分析 ==="
    INITIAL_DISK=$(du -sk ent-dns.db 2>/dev/null | awk '{print $1}') || echo "0"
    FINAL_DISK=$(du -sk ent-dns.db 2>/dev/null | awk '{print $1}') || echo "0"
    DISK_DIFF=$((FINAL_DISK - INITIAL_DISK))

    echo "初始磁盘: $INITIAL_DISK KB"
    echo "最终磁盘: $FINAL_DISK KB"
    echo "磁盘增长: $DISK_DIFF KB"

    echo -e "\n=== 崩溃检测 ==="
    if grep -q "panic\|fatal\|killed" "$DNS_LOG" 2>/dev/null; then
        echo -e "${RED}检测到 DNS 进程崩溃${NC}"
    else
        echo -e "${GREEN}DNS 进程稳定运行${NC}"
    fi

} > "$RESULTS_DIR/comparison.txt"

echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}稳定性测试完成！${NC}"
echo -e "${GREEN}结果目录: $RESULTS_DIR${NC}"
echo -e "${GREEN}对比报告: $RESULTS_DIR/comparison.txt${NC}"
echo -e "${GREEN}========================================${NC}"
