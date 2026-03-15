#!/usr/bin/env bash
set -euo pipefail

# 切换到 backend 根目录（脚本从任意位置调用都能正常工作）
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$ROOT_DIR"

BINARY="./target/release/rust-dns"
LOG_FILE="./backend.log"
HEALTH_URL="http://localhost:8080/health"
HEALTH_TIMEOUT=30

# 1. Kill 当前运行的 rust-dns 进程
echo "[1/4] Stopping existing rust-dns processes..."
if pgrep -f "target/release/rust-dns" > /dev/null 2>&1; then
    pkill -f "target/release/rust-dns" || true
    # 等待进程真正退出
    for i in $(seq 1 10); do
        pgrep -f "target/release/rust-dns" > /dev/null 2>&1 || break
        sleep 0.5
    done
    echo "      Stopped."
else
    echo "      No running process found."
fi

# 2. Build release binary
echo "[2/4] Building release binary..."
if ! cargo build --release 2>&1; then
    echo "[ERROR] cargo build --release failed."
    exit 1
fi
echo "      Build succeeded."

# 3. Start binary in background, append logs to backend.log
echo "[3/4] Starting rust-dns..."
nohup "$BINARY" >> "$LOG_FILE" 2>&1 &
NEW_PID=$!
echo "      PID: $NEW_PID"

# 4. Health check (poll /health, max HEALTH_TIMEOUT seconds)
echo "[4/4] Waiting for service to be healthy (max ${HEALTH_TIMEOUT}s)..."
ELAPSED=0
while [ "$ELAPSED" -lt "$HEALTH_TIMEOUT" ]; do
    HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" "$HEALTH_URL" 2>/dev/null || true)
    if [ "$HTTP_CODE" = "200" ]; then
        echo ""
        echo "Service is healthy. (${ELAPSED}s)"
        echo "Log file: $LOG_FILE"
        exit 0
    fi
    sleep 1
    ELAPSED=$((ELAPSED + 1))
    printf "."
done

echo ""
echo "[ERROR] Service did not become healthy within ${HEALTH_TIMEOUT}s."
echo "Check logs: tail -f $LOG_FILE"
exit 1
