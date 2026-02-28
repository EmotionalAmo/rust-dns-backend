#!/bin/bash

# DNS 压力测试脚本
# 执行 3 分钟，持续进行 DNS 查询

set -e

DNS_PORT="${DNS_PORT:-15353}"
DURATION="${DURATION:-180}"  # 3 分钟 (180 秒)
LOG_FILE="/tmp/dns-stress-results.log"

# 常见域名列表（用于测试）
DOMAINS=(
    "google.com"
    "github.com"
    "stackoverflow.com"
    "reddit.com"
    "twitter.com"
    "facebook.com"
    "amazon.com"
    "apple.com"
    "microsoft.com"
    "netflix.com"
    "spotify.com"
    "discord.com"
    "zoom.us"
    "slack.com"
    "dropbox.com"
    "cloudflare.com"
    "aws.amazon.com"
    "baidu.com"
    "taobao.com"
    "tmall.com"
)

# 清空日志文件
: > "$LOG_FILE"

# 统计变量
TOTAL_QUERIES=0
SUCCESS_COUNT=0
ERROR_COUNT=0
START_TIME=$(date +%s)

echo "========================================="
echo "DNS 压力测试"
echo "========================================="
echo "DNS 端口: $DNS_PORT"
echo "测试时长: $DURATION 秒 ($((DURATION / 60)) 分钟)"
echo "域名数量: ${#DOMAINS[@]}"
echo "开始时间: $(date)"
echo "========================================="
echo ""

# 测试函数
query_dns() {
    local domain="$1"
    local start_time=$(date +%s%3N)

    if dig @"127.0.0.1" -p "$DNS_PORT" "$domain" A +short +time=2 >/dev/null 2>&1; then
        local end_time=$(date +%s%3N)
        local duration=$(echo "$end_time - $start_time" | bc)
        echo "[$(date '+%H:%M:%S')] ✅ $domain - ${duration}ms" >> "$LOG_FILE"
        echo "success" >&3
    else
        echo "[$(date '+%H:%M:%S')] ❌ $domain - ERROR" >> "$LOG_FILE"
        echo "error" >&3
    fi
}

# 使用多个并发 worker
MAX_WORKERS=10
worker_pids=()

# 后台进程：持续运行查询
run_test() {
    local end_time=$((START_TIME + DURATION))

    while [ $(date +%s) -lt $end_time ]; do
        for domain in "${DOMAINS[@]}"; do
            # 检查是否超时
            [ $(date +%s) -ge $end_time ] && break 2

            # 等待可用 worker
            while [ ${#worker_pids[@]} -ge $MAX_WORKERS ]; do
                new_pids=()
                for pid in "${worker_pids[@]}"; do
                    if kill -0 $pid 2>/dev/null; then
                        new_pids+=($pid)
                    fi
                done
                worker_pids=("${new_pids[@]}")
                if [ ${#worker_pids[@]} -ge $MAX_WORKERS ]; then
                    sleep 0.01
                fi
            done

            # 启动查询（后台）
            {
                local start_time=$(date +%s%3N)
                if dig @"127.0.0.1" -p "$DNS_PORT" "$domain" A +short +time=2 >/dev/null 2>&1; then
                    local end_time=$(date +%s%3N)
                    local duration=$(echo "scale=2; $end_time - $start_time" | bc 2>/dev/null || echo "0.00")
                    echo "[$(date '+%H:%M:%S')] ✅ $domain - ${duration}ms" >> "$LOG_FILE"
                else
                    echo "[$(date '+%H:%M:%S')] ❌ $domain - ERROR" >> "$LOG_FILE"
                fi
            } &
            worker_pids+=($!)
        done
    done
}

# 启动测试
echo "开始 DNS 查询..."
run_test &
MAIN_PID=$!

# 显示实时统计
update_stats() {
    local elapsed=$(( $(date +%s) - START_TIME ))
    local remaining=$((DURATION - elapsed))

    # 统计日志文件中的成功/失败数
    local success=$(grep -c "✅" "$LOG_FILE" 2>/dev/null) || success=0
    local error=$(grep -c "❌" "$LOG_FILE" 2>/dev/null) || error=0
    local total=$((success + error))

    # 清屏并显示统计
    clear
    echo "========================================="
    echo "DNS 压力测试 - 实时统计"
    echo "========================================="
    echo "已运行: ${elapsed}s / ${DURATION}s"
    echo "剩余时间: ${remaining}s"
    echo "========================================="
    echo "总查询数: $total"
    echo "成功: $success ($(echo "scale=1; $success * 100 / ($total + 1)" | bc 2>/dev/null || echo 0)%)"
    echo "失败: $error ($(echo "scale=1; $error * 100 / ($total + 1)" | bc 2>/dev/null || echo 0)%)"
    echo "========================================="
    echo "实时 QPS: $(echo "scale=1; $total / ($elapsed + 1)" | bc 2>/dev/null || echo 0)"
    echo "========================================="
    echo ""
    echo "最近查询:"
    tail -10 "$LOG_FILE" 2>/dev/null || echo "(暂无查询记录)"
}

# 每秒更新统计
while [ $(date +%s) -lt $((START_TIME + DURATION)) ]; do
    update_stats
    sleep 1
done

# 等待主测试进程完成
wait $MAIN_PID 2>/dev/null || true

# 最终统计
END_TIME=$(date +%s)
ELAPSED=$((END_TIME - START_TIME))
FINAL_SUCCESS=$(grep -c "✅" "$LOG_FILE" 2>/dev/null) || FINAL_SUCCESS=0
FINAL_ERROR=$(grep -c "❌" "$LOG_FILE" 2>/dev/null) || FINAL_ERROR=0
FINAL_TOTAL=$((FINAL_SUCCESS + FINAL_ERROR))

echo ""
echo "========================================="
echo "测试完成!"
echo "========================================="
echo "总时长: ${ELAPSED}s"
echo "总查询数: $FINAL_TOTAL"
echo "成功: $FINAL_SUCCESS"
echo "失败: $FINAL_ERROR"
echo "平均 QPS: $(echo "scale=2; $FINAL_TOTAL / $ELAPSED" | bc 2>/dev/null || echo 0)"
echo "========================================="
echo "详细日志: $LOG_FILE"
echo "========================================="
