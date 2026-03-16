#!/usr/bin/env bash
# ================================================================
# rust-dns Production Deploy Script
# 用法: bash /opt/ent-dns/deploy.sh [--skip-backup]
# ================================================================
set -euo pipefail

DEPLOY_DIR="/opt/ent-dns"
REPO_DIR="${DEPLOY_DIR}/rust-dns-backend"
BIN_DIR="${DEPLOY_DIR}/bin"
BACKUP_DIR="${DEPLOY_DIR}/backups"
SERVICE_NAME="rust-dns"
BINARY="${BIN_DIR}/rust-dns"

# 颜色输出
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
info()  { echo -e "${GREEN}[INFO]${NC}  $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC}  $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*" >&2; exit 1; }

SKIP_BACKUP=false
[[ "${1:-}" == "--skip-backup" ]] && SKIP_BACKUP=true

# ── 1. 前置检查 ─────────────────────────────────────────────────
info "Step 1/6: Pre-flight checks"

# 磁盘空间检查（最低 5 GB）
AVAIL_GB=$(df -BG /opt | awk 'NR==2 {gsub("G",""); print $4}')
if [[ "${AVAIL_GB}" -lt 5 ]]; then
    error "磁盘空间不足: ${AVAIL_GB}GB 可用，最低需要 5GB"
fi
info "磁盘空间 OK: ${AVAIL_GB}GB 可用"

# ── 2. 备份数据库 ────────────────────────────────────────────────
info "Step 2/6: Database backup"
mkdir -p "${BACKUP_DIR}"

if [[ "${SKIP_BACKUP}" == "false" ]]; then
    TIMESTAMP=$(date +%Y%m%d_%H%M%S)
    BACKUP_FILE="${BACKUP_DIR}/rust_dns_${TIMESTAMP}.sql.gz"
    DB_URL=$(sudo grep DATABASE_URL /etc/rust-dns/env 2>/dev/null | cut -d= -f2- || echo "")

    if [[ -n "${DB_URL}" ]]; then
        pg_dump "${DB_URL}" | gzip > "${BACKUP_FILE}"
        BACKUP_SIZE=$(du -sh "${BACKUP_FILE}" | cut -f1)
        info "备份完成: ${BACKUP_FILE} (${BACKUP_SIZE})"
    else
        warn "未找到 DATABASE_URL，跳过数据库备份"
    fi

    # 只保留最近 7 个备份
    ls -t "${BACKUP_DIR}"/rust_dns_*.sql.gz 2>/dev/null | tail -n +8 | xargs -r rm -f
else
    warn "跳过备份（--skip-backup 模式）"
fi

# ── 3. 拉取代码 ──────────────────────────────────────────────────
info "Step 3/6: Git pull"
cd "${REPO_DIR}"
git fetch origin
git pull origin main
COMMIT=$(git rev-parse --short HEAD)
info "当前 commit: ${COMMIT}"

# ── 4. 编译 ──────────────────────────────────────────────────────
info "Step 4/6: cargo build --release (ARM 平台约 13 分钟...)"
BUILD_LOG="/tmp/rust-dns-build-$(date +%Y%m%d_%H%M%S).log"
cargo build --release 2>&1 | tee "${BUILD_LOG}"

NEW_BINARY="${REPO_DIR}/target/release/rust-dns"
[[ -f "${NEW_BINARY}" ]] || error "编译失败，二进制未生成。查看日志: ${BUILD_LOG}"
info "编译成功: $(ls -lh "${NEW_BINARY}" | awk '{print $5}')"

# ── 5. 部署 ──────────────────────────────────────────────────────
info "Step 5/6: Deploy"

# 保存旧版本
mkdir -p "${BIN_DIR}"
[[ -f "${BINARY}" ]] && sudo cp "${BINARY}" "${BINARY}.prev"

# 停止服务
sudo systemctl stop "${SERVICE_NAME}"
info "服务已停止"

# 替换二进制
sudo cp "${NEW_BINARY}" "${BINARY}"
sudo chmod 755 "${BINARY}"
sudo setcap 'cap_net_bind_service=+ep' "${BINARY}"

# 启动服务
sudo systemctl start "${SERVICE_NAME}"
sleep 5

# ── 6. 验证 ──────────────────────────────────────────────────────
info "Step 6/6: Smoke test"

# 先做 systemd 快速检查，失败立即回滚（不等 smoke-test）
if ! sudo systemctl is-active --quiet "${SERVICE_NAME}"; then
    error "服务启动失败！执行回滚..."
    sudo cp "${BINARY}.prev" "${BINARY}" 2>/dev/null || true
    sudo setcap 'cap_net_bind_service=+ep' "${BINARY}" 2>/dev/null || true
    sudo systemctl start "${SERVICE_NAME}"
    error "已回滚，请检查日志: journalctl -u ${SERVICE_NAME} -n 50"
fi

info "服务状态: $(sudo systemctl is-active ${SERVICE_NAME})"

# 调用完整 smoke-test（7 项检查）
SMOKE_SCRIPT="${REPO_DIR}/scripts/smoke-test.sh"
if [[ -f "${SMOKE_SCRIPT}" ]]; then
    info "运行完整 smoke test..."
    if bash "${SMOKE_SCRIPT}"; then
        info "Smoke test 全部通过"
    else
        warn "Smoke test 存在失败项，请检查上方输出。服务已运行，但建议手动排查后再放行流量。"
    fi
else
    warn "未找到 ${SMOKE_SCRIPT}，跳过完整 smoke test"
    # 兜底：基础 HTTP + DNS 检查
    HTTP_CODE=$(curl -sf -o /dev/null -w "%{http_code}" http://localhost:8080/api/health 2>/dev/null || echo "000")
    if [[ "${HTTP_CODE}" == "200" ]]; then
        info "健康检查 OK (HTTP ${HTTP_CODE})"
    else
        warn "健康检查返回 HTTP ${HTTP_CODE}（如无 /api/health 接口可忽略）"
    fi
    if dig @127.0.0.1 -p 53 cloudflare.com A +short +time=3 &>/dev/null; then
        info "DNS 解析 OK"
    else
        warn "DNS 解析检查失败，请手动确认"
    fi
fi

echo ""
info "=== 部署完成 ==="
info "版本 commit: ${COMMIT}"
info "查看日志: journalctl -u ${SERVICE_NAME} -f"
