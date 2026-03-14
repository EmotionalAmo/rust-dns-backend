#!/bin/bash
# rust-dns PostgreSQL Backup Script
# Usage: ./scripts/backup.sh [backup_dir]
# Requires pg_dump (part of postgresql-client)
#
# Database URL is read from the first of:
#   1. RUST_DNS__DATABASE__URL env var
#   2. DATABASE_URL env var
#   3. Built-in default (localhost dev)

set -e

BACKUP_DIR="${1:-./backups}"
DATABASE_URL="${RUST_DNS__DATABASE__URL:-${DATABASE_URL:-postgres://postgres:postgres@localhost:5432/rustdns}}"
MAX_AGE_DAYS=7

mkdir -p "${BACKUP_DIR}"

echo "[$(date '+%Y-%m-%d %H:%M:%S')] Starting rust-dns backup..."
echo "[$(date '+%Y-%m-%d %H:%M:%S')] Backup dir: ${BACKUP_DIR}"

TIMESTAMP=$(date '+%Y%m%d-%H%M%S')
BACKUP_FILE="${BACKUP_DIR}/rust-dns-backup-${TIMESTAMP}.dump"

pg_dump --format=custom --no-password --file="${BACKUP_FILE}" "${DATABASE_URL}"

echo "[$(date '+%Y-%m-%d %H:%M:%S')] Backup created: ${BACKUP_FILE} ($(du -h "${BACKUP_FILE}" | cut -f1))"

# Remove old backups
echo "[$(date '+%Y-%m-%d %H:%M:%S')] Cleaning up backups older than ${MAX_AGE_DAYS} days..."
find "${BACKUP_DIR}" -name "rust-dns-backup-*.dump" -type f -mtime +"${MAX_AGE_DAYS}" -delete

COUNT=$(find "${BACKUP_DIR}" -name "rust-dns-backup-*.dump" -type f | wc -l)
echo "[$(date '+%Y-%m-%d %H:%M:%S')] Backup complete. ${COUNT} backup(s) retained."
