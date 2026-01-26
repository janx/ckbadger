#!/bin/bash

set -e

API_URL="${API_URL:-http://localhost:3001}"
TEST_TYPE="${1:-quick}"

echo "================================"
echo "CKBEYE API Load Testing"
echo "================================"
echo "API URL: $API_URL"
echo "Test Type: $TEST_TYPE"
echo ""

check_api() {
    echo "Checking API availability..."
    if ! curl -s -o /dev/null -w "%{http_code}" "$API_URL/api/v1/statistics/network" | grep -q "200"; then
        echo "ERROR: API is not responding at $API_URL"
        exit 1
    fi
    echo "API is available"
    echo ""
}

run_k6_quick() {
    echo "Running k6 quick test (30s, 10 VUs)..."
    k6 run --env API_URL="$API_URL" scripts/load-test-quick.js
}

run_k6_full() {
    echo "Running k6 full test suite (smoke + load + stress)..."
    k6 run --env API_URL="$API_URL" scripts/load-test.js
}

run_wrk() {
    echo "Running wrk benchmark..."
    
    if ! command -v wrk &> /dev/null; then
        echo "wrk not installed, skipping..."
        return
    fi
    
    echo ""
    echo "--- Blocks Endpoint ---"
    wrk -t4 -c100 -d30s "$API_URL/api/v1/blocks?limit=10"
    
    echo ""
    echo "--- Transactions Endpoint ---"
    wrk -t4 -c100 -d30s "$API_URL/api/v1/transactions?limit=10"
    
    echo ""
    echo "--- Statistics Endpoint ---"
    wrk -t4 -c100 -d30s "$API_URL/api/v1/statistics/network"
    
    echo ""
    echo "--- Mixed Workload (Lua Script) ---"
    wrk -t4 -c100 -d30s -s scripts/wrk-test.lua "$API_URL"
}

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
        echo ""
        echo "================================"
        echo ""
        run_wrk
        ;;
    *)
        echo "Usage: $0 [quick|full|wrk|all]"
        echo ""
        echo "  quick - 30 second k6 test with 10 VUs (default)"
        echo "  full  - Full k6 test suite with smoke, load, and stress tests"
        echo "  wrk   - wrk benchmark against all endpoints"
        echo "  all   - Run both k6 quick and wrk tests"
        exit 1
        ;;
esac

echo ""
echo "================================"
echo "Load testing complete!"
echo "================================"
