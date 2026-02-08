#!/bin/bash

set -e

# ClickHouse INSERT Performance Benchmark Script
# Measures INSERT performance for ckbadger indexer tables

BLOCKS="${1:-1000}"
OUTPUT_FILE=""
CLICKHOUSE_URL="${CLICKHOUSE_URL:-http://localhost:8123}"
CLICKHOUSE_DB="${CLICKHOUSE_DB:-ckbadger}"
DOCKER_CONTAINER="ckbadger-clickhouse"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

print_help() {
    cat << EOF
Usage: $0 [OPTIONS]

Benchmark ClickHouse INSERT performance for ckbadger indexer tables.

Measures INSERT time, memory usage, and row counts from system.query_log.

OPTIONS:
  --blocks N              Number of blocks to analyze (default: 1000)
  --output FILE           Output JSON file path (default: benchmark_<timestamp>.json)
  --clickhouse-url URL    ClickHouse HTTP URL (default: http://localhost:8123)
  --clickhouse-db DB      ClickHouse database name (default: ckbadger)
  --docker CONTAINER      Docker container name (default: ckbadger-clickhouse)
  --help                  Show this help message

ENVIRONMENT VARIABLES:
  CLICKHOUSE_URL         ClickHouse HTTP URL
  CLICKHOUSE_DB          ClickHouse database name
  DOCKER_CONTAINER       Docker container name for ClickHouse

EXAMPLES:
  # Benchmark last 1000 blocks (default)
  ./scripts/benchmark-sync.sh

  # Benchmark last 10000 blocks
  ./scripts/benchmark-sync.sh --blocks 10000

  # Custom output file
  ./scripts/benchmark-sync.sh --output my_benchmark.json

  # Custom ClickHouse URL
  ./scripts/benchmark-sync.sh --clickhouse-url http://ch.example.com:8123

NOTES:
  - Requires ClickHouse to be running and accessible
  - Queries system.query_log for INSERT metrics
  - Queries system.parts for active parts count
  - Outputs JSON format for easy parsing
  - Requires jq for JSON formatting (falls back to plain output)

OUTPUT FORMAT:
  {
    "timestamp": "2024-01-15T10:30:45Z",
    "blocks_analyzed": 1000,
    "duration_seconds": 45.2,
    "tables": {
      "transactions_all": {
        "insert_time_ms": 1234,
        "memory_bytes": 5678900,
        "rows_written": 45000
      },
      ...
    },
    "parts": {
      "transactions_all": { "active_parts": 42 }
    },
    "summary": {
      "sync_speed_blocks_per_sec": 22.1,
      "total_insert_time_ms": 8900,
      "total_memory_bytes": 45678900
    }
  }

EOF
}

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1" >&2
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1" >&2
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1" >&2
}

log_debug() {
    echo -e "${BLUE}[DEBUG]${NC} $1" >&2
}

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --blocks)
            BLOCKS="$2"
            shift 2
            ;;
        --output)
            OUTPUT_FILE="$2"
            shift 2
            ;;
        --clickhouse-url)
            CLICKHOUSE_URL="$2"
            shift 2
            ;;
        --clickhouse-db)
            CLICKHOUSE_DB="$2"
            shift 2
            ;;
        --docker)
            DOCKER_CONTAINER="$2"
            shift 2
            ;;
        --help)
            print_help
            exit 0
            ;;
        *)
            log_error "Unknown option: $1"
            print_help
            exit 1
            ;;
    esac
done

# Set default output file if not provided
if [ -z "$OUTPUT_FILE" ]; then
    TIMESTAMP=$(date +%Y%m%d_%H%M%S)
    OUTPUT_FILE="benchmark_${TIMESTAMP}.json"
fi

log_info "ClickHouse INSERT Performance Benchmark"
log_info "========================================"
log_info "Blocks to analyze: $BLOCKS"
log_info "ClickHouse URL: $CLICKHOUSE_URL"
log_info "Database: $CLICKHOUSE_DB"
log_info "Output file: $OUTPUT_FILE"
echo "" >&2

# Check if ClickHouse is accessible
log_info "Checking ClickHouse connectivity..."
if ! curl -s "$CLICKHOUSE_URL/ping" > /dev/null 2>&1; then
    log_error "Cannot connect to ClickHouse at $CLICKHOUSE_URL"
    log_info "Make sure ClickHouse is running and accessible"
    exit 1
fi
log_info "ClickHouse is accessible"
echo "" >&2

# Function to run ClickHouse query
run_query() {
    local query="$1"
    curl -s "$CLICKHOUSE_URL" \
        --data-binary "$query" \
        -H "X-ClickHouse-Database: $CLICKHOUSE_DB" \
        2>/dev/null || echo ""
}

# Get the current tip block number
log_info "Fetching current tip block number..."
TIP_BLOCK=$(run_query "SELECT MAX(number) FROM blocks_all FORMAT TabSeparated" | tr -d '\n')

if [ -z "$TIP_BLOCK" ] || [ "$TIP_BLOCK" = "0" ]; then
    log_error "Could not fetch tip block number. Database may be empty."
    exit 1
fi

log_info "Current tip block: $TIP_BLOCK"

# Calculate start block
START_BLOCK=$((TIP_BLOCK - BLOCKS + 1))
if [ "$START_BLOCK" -lt 0 ]; then
    START_BLOCK=0
fi

log_info "Analyzing blocks $START_BLOCK to $TIP_BLOCK"
echo "" >&2

# Start timing
BENCHMARK_START=$(date +%s%N)

# Query INSERT metrics from system.query_log
log_info "Querying INSERT metrics from system.query_log..."

# Get INSERT metrics for each table
QUERY_LOG_QUERY="
SELECT
    table,
    COUNT(*) as insert_count,
    SUM(query_duration_ms) as total_insert_time_ms,
    SUM(memory_usage) as total_memory_bytes,
    SUM(read_rows) as total_rows_written
FROM system.query_log
WHERE
    database = '$CLICKHOUSE_DB'
    AND type = 'QueryFinish'
    AND query LIKE 'INSERT INTO %'
    AND event_time > now() - INTERVAL 1 HOUR
GROUP BY table
ORDER BY table
FORMAT JSON
"

INSERT_METRICS=$(run_query "$QUERY_LOG_QUERY")

# Get parts information
log_info "Querying active parts count..."

PARTS_QUERY="
SELECT
    table,
    COUNT(*) as active_parts
FROM system.parts
WHERE
    database = '$CLICKHOUSE_DB'
    AND active = 1
GROUP BY table
ORDER BY table
FORMAT JSON
"

PARTS_INFO=$(run_query "$PARTS_QUERY")

# End timing
BENCHMARK_END=$(date +%s%N)
DURATION_NS=$((BENCHMARK_END - BENCHMARK_START))
DURATION_SEC=$(echo "scale=2; $DURATION_NS / 1000000000" | bc 2>/dev/null || echo "0")

log_info "Benchmark completed in ${DURATION_SEC}s"
echo "" >&2

# Calculate summary statistics
log_info "Calculating summary statistics..."

TOTAL_INSERT_TIME=$(echo "$INSERT_METRICS" | jq -r '[.data[].total_insert_time_ms // 0] | add' 2>/dev/null || echo "0")
TOTAL_MEMORY=$(echo "$INSERT_METRICS" | jq -r '[.data[].total_memory_bytes // 0] | add' 2>/dev/null || echo "0")
TOTAL_ROWS=$(echo "$INSERT_METRICS" | jq -r '[.data[].total_rows_written // 0] | add' 2>/dev/null || echo "0")

BLOCKS_ANALYZED=$((TIP_BLOCK - START_BLOCK + 1))
if [ "$DURATION_SEC" != "0" ]; then
    SYNC_SPEED=$(echo "scale=2; $BLOCKS_ANALYZED / $DURATION_SEC" | bc 2>/dev/null || echo "0")
else
    SYNC_SPEED="0"
fi

# Build JSON output
TIMESTAMP=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

# Create JSON structure
JSON_OUTPUT=$(cat <<EOF
{
  "timestamp": "$TIMESTAMP",
  "blocks_analyzed": $BLOCKS_ANALYZED,
  "start_block": $START_BLOCK,
  "end_block": $TIP_BLOCK,
  "duration_seconds": $DURATION_SEC,
  "tables": $(echo "$INSERT_METRICS" | jq -r '.data | map({(.table): {insert_time_ms: (.total_insert_time_ms // 0), memory_bytes: (.total_memory_bytes // 0), rows_written: (.total_rows_written // 0)}}) | add // {}' 2>/dev/null || echo '{}'),
  "parts": $(echo "$PARTS_INFO" | jq -r '.data | map({(.table): {active_parts: .active_parts}}) | add // {}' 2>/dev/null || echo '{}'),
  "summary": {
    "sync_speed_blocks_per_sec": $SYNC_SPEED,
    "total_insert_time_ms": $TOTAL_INSERT_TIME,
    "total_memory_bytes": $TOTAL_MEMORY,
    "total_rows_written": $TOTAL_ROWS
  }
}
EOF
)

# Format JSON if jq is available
if command -v jq &> /dev/null; then
    JSON_OUTPUT=$(echo "$JSON_OUTPUT" | jq '.' 2>/dev/null || echo "$JSON_OUTPUT")
fi

# Write to file
echo "$JSON_OUTPUT" > "$OUTPUT_FILE"

log_info "Results saved to: $OUTPUT_FILE"
echo "" >&2

# Display summary
log_info "Summary:"
log_info "  Blocks analyzed: $BLOCKS_ANALYZED"
log_info "  Block range: $START_BLOCK - $TIP_BLOCK"
log_info "  Benchmark duration: ${DURATION_SEC}s"
log_info "  Sync speed: ${SYNC_SPEED} blocks/sec"
log_info "  Total INSERT time: ${TOTAL_INSERT_TIME}ms"
log_info "  Total memory used: ${TOTAL_MEMORY} bytes"
log_info "  Total rows written: ${TOTAL_ROWS}"
echo "" >&2

# Display table breakdown if available
if [ -n "$INSERT_METRICS" ] && [ "$INSERT_METRICS" != "{}" ]; then
    log_info "Table breakdown:"
    echo "$INSERT_METRICS" | jq -r '.data[] | "  \(.table): \(.total_insert_time_ms)ms, \(.total_memory_bytes) bytes, \(.total_rows_written) rows"' 2>/dev/null || true
    echo "" >&2
fi

log_info "Full results in JSON format:"
echo "$JSON_OUTPUT" >&2

exit 0
