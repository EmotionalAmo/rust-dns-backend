#!/usr/bin/env bash
# ================================================================
# Ent-DNS Enterprise — Installation Script
# Tested on: Ubuntu 22.04 / Debian 12
# Run as root: sudo bash install.sh
# ================================================================
set -euo pipefail

INSTALL_DIR="/opt/ent-dns"
DATA_DIR="/var/lib/ent-dns"
CONFIG_DIR="/etc/ent-dns"
SERVICE_NAME="ent-dns"
BINARY_NAME="ent-dns"

# --- Sanity checks ---
if [[ $EUID -ne 0 ]]; then
    echo "ERROR: This script must be run as root." >&2
    exit 1
fi

if [[ ! -f "./${BINARY_NAME}" ]]; then
    echo "ERROR: Binary './${BINARY_NAME}' not found in current directory." >&2
    echo "       Build it first: cargo build --release && cp target/release/ent-dns ." >&2
    exit 1
fi

echo "==> Installing Ent-DNS Enterprise..."

# --- Create system user ---
if ! id "${SERVICE_NAME}" &>/dev/null; then
    echo "==> Creating system user '${SERVICE_NAME}'..."
    useradd --system --no-create-home --shell /sbin/nologin "${SERVICE_NAME}"
fi

# --- Create directories ---
echo "==> Creating directories..."
install -d -m 755 "${INSTALL_DIR}"
install -d -m 750 -o "${SERVICE_NAME}" -g "${SERVICE_NAME}" "${DATA_DIR}"
install -d -m 750 "${CONFIG_DIR}"

# --- Install binary ---
echo "==> Installing binary to ${INSTALL_DIR}/${BINARY_NAME}..."
install -m 755 "./${BINARY_NAME}" "${INSTALL_DIR}/${BINARY_NAME}"

# --- Install config template ---
if [[ ! -f "${CONFIG_DIR}/env" ]]; then
    echo "==> Creating config file ${CONFIG_DIR}/env..."
    cat > "${CONFIG_DIR}/env" <<'EOF'
ENT_DNS__DATABASE__PATH=/var/lib/ent-dns/ent-dns.db
ENT_DNS__DNS__BIND=0.0.0.0
ENT_DNS__DNS__PORT=53
ENT_DNS__API__BIND=0.0.0.0
ENT_DNS__API__PORT=8080
ENT_DNS__AUTH__JWT_SECRET=CHANGE_ME_REPLACE_WITH_RANDOM_SECRET
ENT_DNS__AUTH__JWT_EXPIRY_HOURS=24
EOF
    chmod 640 "${CONFIG_DIR}/env"
    chown root:"${SERVICE_NAME}" "${CONFIG_DIR}/env"
    echo ""
    echo "  IMPORTANT: Edit ${CONFIG_DIR}/env and set a strong JWT_SECRET!"
    echo "  Run: openssl rand -hex 32"
    echo ""
fi

# --- Install systemd service ---
echo "==> Installing systemd service..."
install -m 644 "$(dirname "$0")/ent-dns.service" "/etc/systemd/system/${SERVICE_NAME}.service"
systemctl daemon-reload

# --- Set capabilities (allow port 53 without root) ---
echo "==> Setting CAP_NET_BIND_SERVICE on binary..."
setcap 'cap_net_bind_service=+ep' "${INSTALL_DIR}/${BINARY_NAME}"

# --- Enable and start ---
echo "==> Enabling and starting service..."
systemctl enable "${SERVICE_NAME}"
systemctl restart "${SERVICE_NAME}"

echo ""
echo "==> Ent-DNS installed successfully!"
echo "    Status:  systemctl status ${SERVICE_NAME}"
echo "    Logs:    journalctl -u ${SERVICE_NAME} -f"
echo "    Config:  ${CONFIG_DIR}/env"
echo "    Data:    ${DATA_DIR}"
echo ""
echo "    Default admin credentials: admin / admin"
echo "    Change immediately: POST /api/v1/users/{id}/password"
echo ""
