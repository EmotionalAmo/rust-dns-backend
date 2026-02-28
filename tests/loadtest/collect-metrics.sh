#!/usr/bin/env bash
# 指标采集脚本（配合稳定性测试使用）
# 定期采集系统资源、SQLite 状态、Prometheus metrics

set -e

# 配置
RESULTS_DIR="${RESULTS_DIR:-$(dirname "$0")/../results/metrics-$(date +%Y%m%d-%H%M%S)}"
INTERVAL="${INTERVAL:-10}"  # 采集间隔（秒）
DURATION="${DURATION:-86400}"  # 采集总时长（秒）

# 颜色输出
GREEN='\033[0;32m'
NC='\033[0m'

echo -e "${GREEN}=== 指标采集服务 ===${NC}"
echo "采集间隔: ${INTERVAL} 秒"
echo "采集时长: ${DURATION} 秒"
echo "结果目录: ${RESULTS_DIR}"
echo ""

mkdir -p "$RESULTS_DIR"

START_TIME=$(date +%s)
END_TIME=$((START_TIME + DURATION))

while true; do
    CURRENT_TIME=$(date +%s)

    if [ $CURRENT_TIME -ge $END_TIME ]; then
        echo "采集完成"
        break
    fi

    TIMESTAMP=$(date -Iseconds)
    LOG_FILE="$RESULTS_DIR/metrics-$(date +%Y%m%d-%H%M%S).log"

    {
        echo "=== Timestamp: $TIMESTAMP ==="

        echo -e "\n[系统资源]"
        # CPU 和内存
        if command -v top &> /dev/null; then
            top -l 1 -n 10 -pid $(pgrep ent-dns | head -n 1) 2>/dev/null || top -l 1 | head -n 10
        fi

        echo -e "\n[进程信息]"
        ps aux | grep ent-dns | grep -v grep || echo "未找到运行中的 ent-dns 进程"

        echo -e "\n[SQLite 状态]"
        sqlite3 ent-dns.db <<EOF 2>/dev/null
.mode column
SELECT "查询总数:" || COUNT(*) FROM query_log;
SELECT "表页数:" || page_count FROM pragma_page_count;
SELECT "页大小:" || page_size FROM pragma_page_size;
SELECT "WAL 文件大小:" || wal_checkpoint(PASSIVE) FROM pragma_wal_checkpoint;
PRAGMA lock_status;
PRAGMA journal_mode;
EOF

        echo -e "\n[磁盘使用]"
        du -sh ent-dns.db 2>/dev/null || echo "数据库文件未找到"
        ls -lh ent-dns.db-wal 2>/dev/null || echo "WAL 文件未找到"

        echo -e "\n[Prometheus Metrics]"
        curl -s http://127.0.0.1:8080/metrics | grep -E "ent_dns_" || echo "无法获取 metrics"

        echo -e "\n[网络连接]"
        netstat -an | grep ":5353 " | grep ESTABLISHED | wc -l | xargs -I {} echo "DNS 连接数: {}"
        netstat -an | grep ":8080 " | grep ESTABLISHED | wc -l | xargs -I {} echo "API 连接数: {}"

    } > "$LOG_FILE"

    echo "[$(date +%H:%M:%S)] 采集完成: $LOG_FILE"

    sleep $INTERVAL
done

echo "指标采集服务结束"
