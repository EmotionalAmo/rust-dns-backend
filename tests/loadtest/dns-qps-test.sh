#!/usr/bin/env bash
# DNS QPS 容量测试脚本
# 逐步加压测试：100 → 10000 QPS

set -e

# 配置
DNS_SERVER="${DNS_SERVER:-127.0.0.1}"
DNS_PORT="${DNS_PORT:-5353}"
QPS_LEVELS=(100 500 1000 2000 5000 10000)
DURATION="${DURATION:-300}"  # 每个阶段 5 分钟
QUERY_FILE="${QUERY_FILE:-$(dirname "$0")/domains.txt}"
RESULTS_DIR="${RESULTS_DIR:-$(dirname "$0")/../results/qps-test-$(date +%Y%m%d-%H%M%S)}"

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${GREEN}=== Ent-DNS DNS QPS 容量测试 ===${NC}"
echo "目标服务器: ${DNS_SERVER}:${DNS_PORT}"
echo "测试持续时间: ${DURATION} 秒/阶段"
echo "QPS 级别: ${QPS_LEVELS[@]}"
echo "结果目录: ${RESULTS_DIR}"
echo ""

# 检查依赖
if ! command -v dnsperf &> /dev/null; then
    echo -e "${RED}错误: dnsperf 未安装${NC}"
    echo "安装: brew install dnsperf"
    exit 1
fi

# 检查查询文件
if [ ! -f "$QUERY_FILE" ]; then
    echo -e "${YELLOW}警告: 域名列表不存在，正在生成...${NC}"
    curl -s https://raw.githubusercontent.com/curl/curl/master/docs/examples/html-list.html | \
        grep -oP 'href="https?://[^"]+' | \
        sed 's|https://||g' | \
        sed 's|/.*||g' | \
        sort -u | \
        head -1000 > "$QUERY_FILE"
    echo "已生成 1000 个域名到 $QUERY_FILE"
fi

# 创建结果目录
mkdir -p "$RESULTS_DIR"

# 检查服务器健康
echo "检查服务器健康状态..."
if ! dig @${DNS_SERVER} -p ${DNS_PORT} example.com A +short &> /dev/null; then
    echo -e "${RED}错误: DNS 服务器不可达${NC}"
    exit 1
fi
echo -e "${GREEN}服务器健康检查通过${NC}"
echo ""

# 测试循环
for qps in "${QPS_LEVELS[@]}"; do
    echo -e "${YELLOW}========================================${NC}"
    echo -e "${YELLOW}测试 QPS = $qps (${DURATION} 秒)${NC}"
    echo -e "${YELLOW}========================================${NC}"

    # 启动后台监控
    MONITOR_LOG="$RESULTS_DIR/monitor_${qps}qps.log"
    (
        while true; do
            {
                echo "=== $(date -Iseconds) ==="
                echo "Prometheus Metrics:"
                curl -s http://127.0.0.1:8080/metrics 2>/dev/null | grep -E "ent_dns_queries_total" || echo "无法获取 metrics"
                echo ""
            } >> "$MONITOR_LOG"
            sleep 5
        done
    ) &
    MONITOR_PID=$!

    # 运行 dnsperf
    PERF_LOG="$RESULTS_DIR/dnsperf_${qps}qps.log"
    dnsperf -s $DNS_SERVER -p $DNS_PORT -d $QUERY_FILE \
        -l $DURATION -q $qps -s 1000 \
        > "$PERF_LOG" 2>&1 || true

    # 停止监控
    kill $MONITOR_PID 2>/dev/null || true

    # 提取关键指标
    if [ -f "$PERF_LOG" ]; then
        echo -e "${GREEN}测试完成，提取指标...${NC}"

        # 保存摘要
        {
            echo "=== QPS $qps 测试摘要 ==="
            echo "开始时间: $(date -Iseconds)"
            grep "Queries sent:" "$PERF_LOG" || echo "未找到查询数据"
            grep "Queries per second:" "$PERF_LOG" || echo "未找到 QPS 数据"
            grep "Average Latency (ms):" "$PERF_LOG" || echo "未找到延迟数据"
            echo ""
        } >> "$RESULTS_DIR/summary.txt"

        # 显示实时结果
        echo ""
        echo "当前阶段结果:"
        grep "Queries per second:" "$PERF_LOG" || true
        grep "Average Latency (ms):" "$PERF_LOG" || true
    else
        echo -e "${RED}错误: 测试日志未生成${NC}"
    fi

    # 等待系统恢复
    echo ""
    echo -e "${YELLOW}等待 60 秒让系统恢复...${NC}"
    sleep 60
done

echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}所有测试完成！${NC}"
echo -e "${GREEN}结果目录: $RESULTS_DIR${NC}"
echo -e "${GREEN}========================================${NC}"

# 生成对比报告
{
    echo "=== QPS 测试对比报告 ==="
    echo "生成时间: $(date -Iseconds)"
    echo ""
    echo "QPS | 实际 QPS | 平均延迟(ms) | 错误率"
    echo "---|---------|-------------|--------"
    for qps in "${QPS_LEVELS[@]}"; do
        log="$RESULTS_DIR/dnsperf_${qps}qps.log"
        if [ -f "$log" ]; then
            actual_qps=$(grep "Queries per second:" "$log" | awk '{print $4}' || echo "N/A")
            avg_lat=$(grep "Average Latency (ms):" "$log" | awk '{print $4}' || echo "N/A")
            echo "$qps | $actual_qps | $avg_lat | TBD"
        fi
    done
} > "$RESULTS_DIR/comparison.txt"

echo ""
echo "对比报告: $RESULTS_DIR/comparison.txt"
echo "详细日志: $RESULTS_DIR/"
