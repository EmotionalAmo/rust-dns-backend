#!/bin/bash
# Ent-DNS Automated Backup Script
# Usage: ./scripts/backup.sh [data_dir] [backup_dir]

set -e

DATA_DIR="${1:-./data}"
BACKUP_DIR="${2:-./backups}"
DB_PATH="${DATA_DIR}/ent-dns.db"
MAX_AGE_DAYS=7

# Create backup directory if not exists
mkdir -p "${BACKUP_DIR}"

echo "[$(date '+%Y-%m-%d %H:%M:%S')] Starting Ent-DNS backup..."

# Check if database exists
if [ ! -f "${DB_PATH}" ]; then
    echo "Error: Database file not found at ${DB_PATH}"
    exit 1
fi

# Create backup with timestamp
TIMESTAMP=$(date '+%Y%m%d-%H%M%S')
BACKUP_FILE="${BACKUP_DIR}/ent-dns-backup-${TIMESTAMP}.db"

# Use sqlite3 backup command (safe for WAL mode)
sqlite3 "${DB_PATH}" ".backup '${BACKUP_FILE}'"

if [ $? -eq 0 ]; then
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] Backup created: ${BACKUP_FILE}"
else
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] Backup failed!"
    exit 1
fi

# Remove old backups (older than MAX_AGE_DAYS)
echo "[$(date '+%Y-%m-%d %H:%M:%S')] Cleaning up backups older than ${MAX_AGE_DAYS} days..."
find "${BACKUP_DIR}" -name "ent-dns-backup-*.db" -type f -mtime +${MAX_AGE_DAYS} -delete

COUNT=$(find "${BACKUP_DIR}" -name "ent-dns-backup-*.db" -type f | wc -l)
echo "[$(date '+%Y-%m-%d %H:%M:%S')] Backup complete. ${COUNT} backup(s) retained."
