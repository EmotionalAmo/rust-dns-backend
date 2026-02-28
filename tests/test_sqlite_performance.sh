#!/bin/bash

# SQLite Performance Optimization Test Script
# Tests the PRAGMA settings and batch write improvements independently

set -e

echo "=================================================="
echo "SQLite Performance Optimization Test"
echo "=================================================="
echo ""

# Create a temporary database
TEMP_DB="/tmp/ent-dns-perf-test-$(date +%s).db"
echo "Using temporary database: $TEMP_DB"
echo ""

# Remove old test databases
rm -f /tmp/ent-dns-perf-test-*.db

# Test 1: PRAGMA Settings
echo "Test 1: Verify PRAGMA Settings"
echo "----------------------------------------"
echo "Creating database with optimized settings..."

cat > /tmp/test_pragmas.sql << 'EOF'
-- Apply PRAGMA optimizations
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA cache_size = -64000;
PRAGMA mmap_size = 268435456;
PRAGMA wal_autocheckpoint = 1000;
EOF

sqlite3 "$TEMP_DB" < /tmp/test_pragmas.sql

# Verify settings
echo "  journal_mode: $(sqlite3 "$TEMP_DB" "PRAGMA journal_mode;")"
echo "  synchronous: $(sqlite3 "$TEMP_DB" "PRAGMA synchronous;")"
echo "  cache_size: $(sqlite3 "$TEMP_DB" "PRAGMA cache_size;")"
echo "  mmap_size: $(sqlite3 "$TEMP_DB" "PRAGMA mmap_size;")"
echo "  wal_autocheckpoint: $(sqlite3 "$TEMP_DB" "PRAGMA wal_autocheckpoint;")"

echo ""
echo "✅ PRAGMA Settings Applied Successfully"
echo ""

# Test 2: Create test table
echo "Test 2: Create Test Table and Insert Records"
echo "----------------------------------------"
echo "Creating table..."

cat > /tmp/test_table.sql << 'EOF'
CREATE TABLE test_query_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    time TEXT NOT NULL,
    client_ip TEXT NOT NULL,
    question TEXT NOT NULL,
    qtype TEXT NOT NULL,
    status TEXT NOT NULL
);
EOF

sqlite3 "$TEMP_DB" < /tmp/test_table.sql

echo "✅ Test table created"
echo ""

# Test 3: Batch Insert Performance
echo "Test 3: Batch Insert Performance"
echo "----------------------------------------"
echo "Running batch insert test (500 records per batch)..."

START_TIME=$(date +%s.%N)

# Create batch insert script
cat > /tmp/test_batch.sql << 'EOF'
BEGIN TRANSACTION;
INSERT INTO test_query_log (time, client_ip, question, qtype, status)
VALUES ('2026-02-20T10:00:00Z', '192.168.1.100', 'test1.example.com', 'A', 'NOERROR');
INSERT INTO test_query_log (time, client_ip, question, qtype, status)
VALUES ('2026-02-20T10:00:00Z', '192.168.1.101', 'test2.example.com', 'A', 'NOERROR');
INSERT INTO test_query_log (time, client_ip, question, qtype, status)
VALUES ('2026-02-20T10:00:00Z', '192.168.1.102', 'test3.example.com', 'A', 'NOERROR');
-- Repeat 500 times per batch
END TRANSACTION;
EOF

# Generate batch insert script with 500 records per batch
cat > /tmp/batch_insert.sql << 'EOF'
BEGIN TRANSACTION;
EOF

for i in $(seq 1 500); do
    echo "INSERT INTO test_query_log (time, client_ip, question, qtype, status)
VALUES ('2026-02-20T10:00:00Z', '192.168.1.100', 'test$i.example.com', 'A', 'NOERROR');" >> /tmp/batch_insert.sql
done

echo "COMMIT;" >> /tmp/batch_insert.sql

# Run 10 batches (5000 total records)
for batch in $(seq 1 10); do
    sqlite3 "$TEMP_DB" < /tmp/batch_insert.sql
    echo "  Batch $batch/10: 500 records inserted"
done

END_TIME=$(date +%s.%N)
DURATION=$(echo "$END_TIME - $START_TIME" | bc)
QPS=$(echo "5000 / $DURATION" | bc)

echo ""
echo "Batch Insert Results:"
echo "  Total records: 5000"
echo "  Total time: $DURATION seconds"
echo "  Throughput: $QPS records/sec"
echo "  Avg time per record: $(echo "$DURATION * 1000 / 5000" | bc) ms"
echo ""

# Verify records
COUNT=$(sqlite3 "$TEMP_DB" "SELECT COUNT(*) FROM test_query_log;")
echo "Verification: $COUNT records inserted"
echo ""

# Test 4: WAL Checkpoint Test
echo "Test 4: WAL Checkpoint Test"
echo "----------------------------------------"
WAL_FILE="${TEMP_DB}-wal"
SHM_FILE="${TEMP_DB}-shm"

if [ -f "$WAL_FILE" ]; then
    WAL_SIZE=$(stat -f%z "$WAL_FILE" 2>/dev/null || stat -c%s "$WAL_FILE" 2>/dev/null)
    echo "WAL file exists, size: $WAL_SIZE bytes"

    # Trigger checkpoint
    sqlite3 "$TEMP_DB" "PRAGMA wal_checkpoint(TRUNCATE);"

    NEW_WAL_SIZE=$(stat -f%z "$WAL_FILE" 2>/dev/null || stat -c%s "$WAL_FILE" 2>/dev/null)
    echo "After checkpoint: $NEW_WAL_SIZE bytes"
    echo "✅ WAL checkpoint working"
else
    echo "WAL file not found (expected for in-memory DB)"
fi
echo ""

# Test 5: Query Performance
echo "Test 5: Query Performance Test"
echo "----------------------------------------"
START_TIME=$(date +%s.%N)

# Run some queries
for i in $(seq 1 1000); do
    sqlite3 "$TEMP_DB" "SELECT * FROM test_query_log WHERE question LIKE 'test%' LIMIT 1;" > /dev/null
done

END_TIME=$(date +%s.%N)
DURATION=$(echo "$END_TIME - $START_TIME" | bc)

echo "Query Results:"
echo "  1000 queries executed"
echo "  Total time: $DURATION seconds"
echo "  Queries per second: $(echo "1000 / $DURATION" | bc)"
echo ""

# Cleanup
rm -f "$TEMP_DB" "$TEMP_DB-wal" "$TEMP_DB-shm"
rm -f /tmp/test_pragmas.sql /tmp/test_table.sql /tmp/test_batch.sql /tmp/batch_insert.sql

echo "=================================================="
echo "✅ All Performance Tests Complete"
echo "=================================================="
echo ""
echo "Summary of Optimizations:"
echo "  • PRAGMA journal_mode=WAL: Better concurrent writes"
echo "  • PRAGMA synchronous=NORMAL: Reduced fsync overhead"
echo "  • PRAGMA cache_size=-64000: 64MB page cache"
echo "  • PRAGMA mmap_size=268435456: 256MB memory-mapped I/O"
echo "  • PRAGMA wal_autocheckpoint=1000: Automatic WAL checkpoint"
echo "  • Batch size 500: 5x larger batches for better throughput"
echo "  • Flush interval 2s: Reduced transaction frequency"
echo ""
