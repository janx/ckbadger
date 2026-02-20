#!/bin/bash

set -e

API_URL="${API_URL:-http://localhost:3001}"
TEST_TYPE="quick"
OUTPUT_DIR="${OUTPUT_DIR:-artifacts/perf}"
RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"

LOG_FILE=""
SUMMARY_FILE=""
K6_SUMMARY_FILE=""
WRK_BLOCKS_FILE=""
WRK_TRANSACTIONS_FILE=""
WRK_STATISTICS_FILE=""
WRK_MIXED_FILE=""

print_help() {
    cat << EOF
Usage: $0 [quick|full|wrk|all] [--output-dir DIR]

Run API load tests and emit artifacts under a timestamped run id.

OPTIONS:
  quick              30 second k6 test with 10 VUs (default)
  full               Full k6 test suite with smoke/load/stress
  wrk                wrk benchmark against key endpoints
  all                quick k6 test + wrk benchmark
  --output-dir DIR   Output directory for logs/results (default: artifacts/perf)
  --help             Show this help message

ENVIRONMENT VARIABLES:
  API_URL            Target API base URL (default: http://localhost:3001)
  OUTPUT_DIR         Default output directory (overridden by --output-dir)

EXAMPLES:
  API_URL=http://localhost:3001 ./scripts/run-load-tests.sh quick
  ./scripts/run-load-tests.sh all --output-dir artifacts/perf
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        quick|full|wrk|all)
            TEST_TYPE="$1"
            shift
            ;;
        --output-dir)
            OUTPUT_DIR="$2"
            shift 2
            ;;
        --help|-h)
            print_help
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            print_help
            exit 1
            ;;
    esac
done

mkdir -p "$OUTPUT_DIR"

LOG_FILE="${OUTPUT_DIR}/load_test_${RUN_ID}.log"
SUMMARY_FILE="${OUTPUT_DIR}/load_test_${RUN_ID}.md"
K6_SUMMARY_FILE="${OUTPUT_DIR}/load_test_${RUN_ID}_k6_summary.json"
WRK_BLOCKS_FILE="${OUTPUT_DIR}/load_test_${RUN_ID}_wrk_blocks.txt"
WRK_TRANSACTIONS_FILE="${OUTPUT_DIR}/load_test_${RUN_ID}_wrk_transactions.txt"
WRK_STATISTICS_FILE="${OUTPUT_DIR}/load_test_${RUN_ID}_wrk_statistics.txt"
WRK_MIXED_FILE="${OUTPUT_DIR}/load_test_${RUN_ID}_wrk_mixed.txt"

log() {
    echo "$1" | tee -a "$LOG_FILE"
}

ensure_k6() {
    if ! command -v k6 &> /dev/null; then
        log "ERROR: k6 not installed"
        exit 1
    fi
}

check_api() {
    log "Checking API availability..."
    if ! curl -s -o /dev/null -w "%{http_code}" "$API_URL/api/v1/statistics/network" | grep -q "200"; then
        log "ERROR: API is not responding at $API_URL"
        exit 1
    fi
    log "API is available"
    log ""
}

run_k6_quick() {
    ensure_k6
    log "Running k6 quick test (30s, 10 VUs)..."
    k6 run --summary-export "$K6_SUMMARY_FILE" --env API_URL="$API_URL" scripts/load-test-quick.js | tee -a "$LOG_FILE"
}

run_k6_full() {
    ensure_k6
    log "Running k6 full test suite (smoke + load + stress)..."
    k6 run --summary-export "$K6_SUMMARY_FILE" --env API_URL="$API_URL" scripts/load-test.js | tee -a "$LOG_FILE"
}

run_wrk() {
    log "Running wrk benchmark..."

    if ! command -v wrk &> /dev/null; then
        log "wrk not installed, skipping wrk benchmarks"
        return
    fi

    log ""
    log "--- Blocks Endpoint ---"
    wrk -t4 -c100 -d30s "$API_URL/api/v1/blocks?limit=10" | tee "$WRK_BLOCKS_FILE" | tee -a "$LOG_FILE"

    log ""
    log "--- Transactions Endpoint ---"
    wrk -t4 -c100 -d30s "$API_URL/api/v1/transactions?limit=10" | tee "$WRK_TRANSACTIONS_FILE" | tee -a "$LOG_FILE"

    log ""
    log "--- Statistics Endpoint ---"
    wrk -t4 -c100 -d30s "$API_URL/api/v1/statistics/network" | tee "$WRK_STATISTICS_FILE" | tee -a "$LOG_FILE"

    log ""
    log "--- Mixed Workload (Lua Script) ---"
    wrk -t4 -c100 -d30s -s scripts/wrk-test.lua "$API_URL" | tee "$WRK_MIXED_FILE" | tee -a "$LOG_FILE"
}

write_summary() {
    {
        echo "# API Load Test Summary"
        echo ""
        echo "- Run ID: $RUN_ID"
        echo "- Generated at (UTC): $(date -u +%Y-%m-%dT%H:%M:%SZ)"
        echo "- API URL: \`$API_URL\`"
        echo "- Test type: $TEST_TYPE"
        echo ""
        echo "## Artifacts"
        echo ""

        for file in \
            "$LOG_FILE" \
            "$SUMMARY_FILE" \
            "$K6_SUMMARY_FILE" \
            "$WRK_BLOCKS_FILE" \
            "$WRK_TRANSACTIONS_FILE" \
            "$WRK_STATISTICS_FILE" \
            "$WRK_MIXED_FILE"; do
            if [ -f "$file" ]; then
                echo "- \`$file\`"
            fi
        done
    } > "$SUMMARY_FILE"
}

log "================================"
log "ckbadger API Load Testing"
log "================================"
log "Run ID: $RUN_ID"
log "API URL: $API_URL"
log "Test Type: $TEST_TYPE"
log "Output Dir: $OUTPUT_DIR"
log ""

case "$TEST_TYPE" in
    quick)
        check_api
        run_k6_quick
        ;;
    full)
        check_api
        run_k6_full
        ;;
    wrk)
        check_api
        run_wrk
        ;;
    all)
        check_api
        run_k6_quick
        log ""
        log "================================"
        log ""
        run_wrk
        ;;
    *)
        echo "Usage: $0 [quick|full|wrk|all] [--output-dir DIR]"
        exit 1
        ;;
esac

write_summary

log ""
log "================================"
log "Load testing complete!"
log "Summary: $SUMMARY_FILE"
log "================================"
