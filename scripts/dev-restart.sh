#!/bin/bash
# rust-dns Local Dev Restart Script
# Usage: ./scripts/dev-restart.sh
#
# Kills the running rust-dns process (if any), rebuilds the binary,
# restarts it in the background, and waits for the health endpoint to respond.

set -e

BINARY="./target/release/rust-dns"
LOG_FILE="./backend.log"
HEALTH_URL="http://localhost:8080/health"
HEALTH_TIMEOUT=30

log() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*"
}

# 1. Kill existing process
EXISTING_PID=$(pgrep -f "target/release/rust-dns" 2>/dev/null || true)
if [ -n "$EXISTING_PID" ]; then
    log "Stopping existing process (PID: $EXISTING_PID)..."
    kill "$EXISTING_PID"
    sleep 1
    # Force kill if still running
    if kill -0 "$EXISTING_PID" 2>/dev/null; then
        kill -9 "$EXISTING_PID" 2>/dev/null || true
    fi
    log "Process stopped."
else
    log "No running rust-dns process found."
fi

# 2. Build
log "Building (cargo build --release)..."
cargo build --release

log "Build complete."

# 3. Start in background
log "Starting rust-dns..."
nohup "$BINARY" >> "$LOG_FILE" 2>&1 &
NEW_PID=$!
log "Started (PID: $NEW_PID), logging to $LOG_FILE"

# 4. Wait for health check
log "Waiting for service to become healthy (timeout: ${HEALTH_TIMEOUT}s)..."
for i in $(seq 1 "$HEALTH_TIMEOUT"); do
    if curl -sf "$HEALTH_URL" > /dev/null 2>&1; then
        log "Service is healthy. Done."
        exit 0
    fi
    sleep 1
done

log "ERROR: Service did not become healthy within ${HEALTH_TIMEOUT}s. Check $LOG_FILE for details."
exit 1
