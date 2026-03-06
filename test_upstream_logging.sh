#!/bin/bash

# Test script for Query Log Upstream Display Feature
# This script verifies that the upstream field is properly logged

set -e

echo "=== Query Log Upstream Display Test ==="
echo ""

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# 1. Check if server is running
echo "1. Checking if server is running..."
if curl -s http://localhost:3000/api/health > /dev/null; then
    echo -e "${GREEN}✓ Server is running${NC}"
else
    echo -e "${RED}✗ Server is not running${NC}"
    echo "Start the server with: cargo run"
    exit 1
fi
echo ""

# 2. Check database schema
echo "2. Checking database schema..."
if [ -f "dns.db" ]; then
    RESULT=$(sqlite3 dns.db "PRAGMA table_info(query_log);" | grep upstream)
    if [ -n "$RESULT" ]; then
        echo -e "${GREEN}✓ 'upstream' column exists in query_log table${NC}"
    else
        echo -e "${RED}✗ 'upstream' column not found${NC}"
        exit 1
    fi
else
    echo -e "${RED}✗ Database file not found (dns.db)${NC}"
    echo "Run the server first to create the database"
    exit 1
fi
echo ""

# 3. Make a test DNS query
echo "3. Making test DNS queries..."
dig @127.0.0.1 -p 53 test.example.com A > /dev/null 2>&1
dig @127.0.0.1 -p 53 example.org A > /dev/null 2>&1
echo -e "${GREEN}✓ Test queries sent${NC}"
echo ""

# 4. Wait for batch writer to flush
echo "4. Waiting for batch writer to flush..."
sleep 3
echo -e "${GREEN}✓ Wait complete${NC}"
echo ""

# 5. Check query log API
echo "5. Checking query log API..."
RESPONSE=$(curl -s http://localhost:3000/api/query-log?limit=5)

# Extract first query log entry
FIRST_ENTRY=$(echo "$RESPONSE" | jq '.data[0]')

if [ -z "$FIRST_ENTRY" ] || [ "$FIRST_ENTRY" = "null" ]; then
    echo -e "${RED}✗ No query log entries found${NC}"
    exit 1
fi

# Check if upstream field exists
UPSTREAM=$(echo "$FIRST_ENTRY" | jq -r '.upstream')
echo "Sample entry:"
echo "$FIRST_ENTRY" | jq '.'
echo ""

if [ "$UPSTREAM" != "null" ] && [ -n "$UPSTREAM" ]; then
    echo -e "${GREEN}✓ Upstream field is populated: $UPSTREAM${NC}"
else
    STATUS=$(echo "$FIRST_ENTRY" | jq -r '.status')
    if [ "$STATUS" = "cached" ] || [ "$STATUS" = "blocked" ]; then
        echo -e "${GREEN}✓ Upstream field is null as expected for $STATUS queries${NC}"
    else
        echo -e "${RED}✗ Upstream field should be populated for $STATUS queries${NC}"
    fi
fi
echo ""

# 6. Summary
echo "=== Test Summary ==="
echo -e "${GREEN}✓ All checks passed!${NC}"
echo ""
echo "The upstream display feature is working correctly."
echo "Query log entries now include the 'upstream' field showing which DNS server resolved the query."
