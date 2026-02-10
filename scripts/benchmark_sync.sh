#!/bin/bash

set -e

START_BLOCK=0
END_BLOCK=10000000
CHECKPOINTS="1000000,5000000,10000000"
OUTPUT_FILE="benchmark_results.csv"
QUICK_MODE=false

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

print_help() {
    cat << EOF
Usage: $0 [OPTIONS]

Benchmark CKB indexer sync performance by monitoring block progress and memory usage.

OPTIONS:
  --start N              Starting block number (default: 0)
  --end N                Ending block number (default: 10000000)
  --checkpoints LIST     Comma-separated checkpoint block numbers
                         (default: 1000000,5000000,10000000)
  --quick                Quick mode: sync first 100K blocks only
                         (sets --end 100000 --checkpoints 50000,100000)
  --output FILE          Output CSV file (default: benchmark_results.csv)
  --help                 Show this help message

ENVIRONMENT VARIABLES:
  CKBADGER_DATA_PATH     Path to ckbadger-store RocksDB data (required)
  CKB_RPC_URL            CKB node RPC URL (optional, for reference)

EXAMPLES:
  # Full benchmark with default checkpoints
  ./scripts/benchmark_sync.sh

  # Quick test for CI
  ./scripts/benchmark_sync.sh --quick

  # Custom checkpoints
  ./scripts/benchmark_sync.sh --checkpoints "500000,1000000,2000000"

  # Custom output file
  ./scripts/benchmark_sync.sh --output my_results.csv

NOTES:
  - This script monitors a running indexer process
  - It does NOT start or stop the indexer
  - Requires CKBADGER_DATA_PATH environment variable
  - Outputs CSV format with columns:
    checkpoint, blocks_synced, duration_sec, blocks_per_sec, memory_mb

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

while [[ $# -gt 0 ]]; do
    case $1 in
        --start)
            START_BLOCK="$2"
            shift 2
            ;;
        --end)
            END_BLOCK="$2"
            shift 2
            ;;
        --checkpoints)
            CHECKPOINTS="$2"
            shift 2
            ;;
        --quick)
            QUICK_MODE=true
            END_BLOCK=100000
            CHECKPOINTS="50000,100000"
            shift
            ;;
        --output)
            OUTPUT_FILE="$2"
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

if [ -z "$CKBADGER_DATA_PATH" ]; then
    log_error "CKBADGER_DATA_PATH environment variable is not set"
    exit 1
fi

get_tip_block() {
    # Read tip block from the running indexer's log output or RocksDB
    # The indexer logs tip block periodically; we parse the most recent one
    local pid=$(get_indexer_pid)
    if [ -z "$pid" ]; then
        echo ""
        return
    fi
    # Try reading from /proc/<pid>/fd/1 or fall back to log file
    local log_file="/tmp/ckbadger-indexer.log"
    if [ -f "$log_file" ]; then
        grep -oP 'tip_block_number=\K[0-9]+' "$log_file" 2>/dev/null | tail -1
    else
        echo ""
    fi
}

get_memory_mb() {
    local pid=$1
    if [ -z "$pid" ]; then
        echo "0"
        return
    fi

    if command -v ps &> /dev/null; then
        local rss=$(ps -o rss= -p "$pid" 2>/dev/null || echo "0")
        echo $((rss / 1024))
    else
        echo "0"
    fi
}

get_indexer_pid() {
    pgrep -f "ckbadger-indexer" | head -1
}

if [ ! -f "$OUTPUT_FILE" ]; then
    echo "checkpoint,blocks_synced,duration_sec,blocks_per_sec,memory_mb" > "$OUTPUT_FILE"
    log_info "Created new CSV file: $OUTPUT_FILE"
else
    log_info "Appending to existing CSV file: $OUTPUT_FILE"
fi

log_info "Benchmark Configuration:"
log_info "  Start Block: $START_BLOCK"
log_info "  End Block: $END_BLOCK"
log_info "  Checkpoints: $CHECKPOINTS"
log_info "  Output File: $OUTPUT_FILE"
log_info "  Data Path: $CKBADGER_DATA_PATH"
if [ "$QUICK_MODE" = true ]; then
    log_info "  Mode: QUICK (100K blocks)"
fi
echo "" >&2

IFS=',' read -ra CHECKPOINT_ARRAY <<< "$CHECKPOINTS"

INDEXER_PID=$(get_indexer_pid)
if [ -z "$INDEXER_PID" ]; then
    log_warn "No running ckbadger-indexer process found"
    log_info "Make sure the indexer is running before starting the benchmark"
    log_info "You can start it with: cargo run -p ckbadger-indexer"
    exit 1
fi

log_info "Found indexer process: PID $INDEXER_PID"
echo "" >&2

INITIAL_TIP=$(get_tip_block)
if [ -z "$INITIAL_TIP" ]; then
    log_error "Could not determine current tip block from indexer logs"
    log_info "Make sure the indexer is logging to /tmp/ckbadger-indexer.log"
    exit 1
fi

log_info "Initial tip block: $INITIAL_TIP"
BENCHMARK_START_TIME=$(date +%s)
LAST_CHECKPOINT_BLOCK=$INITIAL_TIP
LAST_CHECKPOINT_TIME=$BENCHMARK_START_TIME

echo "" >&2

for CHECKPOINT in "${CHECKPOINT_ARRAY[@]}"; do
    CHECKPOINT=$((CHECKPOINT))

    log_info "Waiting for checkpoint: $CHECKPOINT blocks..."

    while true; do
        CURRENT_TIP=$(get_tip_block)

        if [ -z "$CURRENT_TIP" ]; then
            log_error "Lost connection to indexer"
            exit 1
        fi

        if [ "$CURRENT_TIP" -ge "$CHECKPOINT" ]; then
            break
        fi

        sleep 1
    done

    CURRENT_TIME=$(date +%s)
    ELAPSED=$((CURRENT_TIME - LAST_CHECKPOINT_TIME))
    BLOCKS_SYNCED=$((CURRENT_TIP - LAST_CHECKPOINT_BLOCK))

    if [ "$ELAPSED" -eq 0 ]; then
        BLOCKS_PER_SEC=0
    else
        BLOCKS_PER_SEC=$((BLOCKS_SYNCED / ELAPSED))
    fi

    MEMORY_MB=$(get_memory_mb "$INDEXER_PID")

    echo "$CHECKPOINT,$CURRENT_TIP,$ELAPSED,$BLOCKS_PER_SEC,$MEMORY_MB" >> "$OUTPUT_FILE"

    log_info "Checkpoint $CHECKPOINT reached"
    log_info "  Blocks synced since last checkpoint: $BLOCKS_SYNCED"
    log_info "  Time elapsed: ${ELAPSED}s"
    log_info "  Blocks/sec: $BLOCKS_PER_SEC"
    log_info "  Memory usage: ${MEMORY_MB}MB"
    echo "" >&2

    LAST_CHECKPOINT_BLOCK=$CURRENT_TIP
    LAST_CHECKPOINT_TIME=$CURRENT_TIME

    if [ "$CURRENT_TIP" -ge "$END_BLOCK" ]; then
        log_info "Reached end block: $END_BLOCK"
        break
    fi
done

TOTAL_TIME=$(($(date +%s) - BENCHMARK_START_TIME))
FINAL_TIP=$(get_tip_block)
TOTAL_BLOCKS=$((FINAL_TIP - INITIAL_TIP))

echo "" >&2
log_info "Benchmark Complete!"
log_info "  Total blocks synced: $TOTAL_BLOCKS"
log_info "  Total time: ${TOTAL_TIME}s"
if [ "$TOTAL_TIME" -gt 0 ]; then
    OVERALL_RATE=$((TOTAL_BLOCKS / TOTAL_TIME))
    log_info "  Overall rate: ${OVERALL_RATE} blocks/sec"
fi
log_info "  Results saved to: $OUTPUT_FILE"
echo "" >&2

log_info "Results:"
cat "$OUTPUT_FILE" >&2
