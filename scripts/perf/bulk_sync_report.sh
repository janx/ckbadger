#!/usr/bin/env bash

set -euo pipefail

LOG_FILE=""
OUTPUT_ROOT="artifacts/perf/bulk-sync"
RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
STATUS="partial"

usage() {
  cat <<'EOF'
Usage: scripts/perf/bulk_sync_report.sh --log FILE [OPTIONS]

Options:
  --log FILE            Cleaned indexer log file to parse (required)
  --output-root DIR     Output root directory (default: artifacts/perf/bulk-sync)
  --run-id ID           Run ID (default: current UTC timestamp)
  --status STATUS       Run status: partial|completed (default: partial)
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --log)
      LOG_FILE="$2"
      shift 2
      ;;
    --output-root)
      OUTPUT_ROOT="$2"
      shift 2
      ;;
    --run-id)
      RUN_ID="$2"
      shift 2
      ;;
    --status)
      STATUS="$2"
      shift 2
      ;;
    --help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [ -z "$LOG_FILE" ]; then
  echo "--log is required" >&2
  exit 1
fi

if [ ! -f "$LOG_FILE" ]; then
  echo "log file not found: $LOG_FILE" >&2
  exit 1
fi

RUN_DIR="${OUTPUT_ROOT}/${RUN_ID}"
LATEST_DIR="${OUTPUT_ROOT}/latest"
mkdir -p "$RUN_DIR" "$LATEST_DIR"

TMP_PARSED="$(mktemp)"
TMP_PENDING="$(mktemp)"
TMP_L0="$(mktemp)"
TMP_IMM="$(mktemp)"
TMP_DATASET="$(mktemp)"
TMP_SECS="$(mktemp)"
TMP_COMMITS="$(mktemp)"
cleanup() {
  rm -f "$TMP_PARSED" "$TMP_PENDING" "$TMP_L0" "$TMP_IMM" "$TMP_DATASET" "$TMP_SECS" "$TMP_COMMITS"
}
trap cleanup EXIT

awk -v pending_file="$TMP_PENDING" -v l0_file="$TMP_L0" -v imm_file="$TMP_IMM" '
{
  line = $0
  if (match(line, /Wrote blocks ([0-9]+) to ([0-9]+) \(([0-9]+) remaining, ([0-9.]+)s, commit=([0-9.]+)ms/, m)) {
    start = m[1] + 0
    end = m[2] + 0
    sec = m[4] + 0
    commit = m[5] + 0
    blocks = end - start + 1
    mode = (index(line, "[BULK]") > 0) ? "bulk" : "all"
    printf "%s\t%.0f\t%.6f\t%.6f\n", mode, blocks, sec, commit
  }
  if (match(line, /compaction_pending_mb=([0-9]+)/, p)) {
    print p[1] > pending_file
  }
  if (match(line, /l0_files=([0-9]+)/, l)) {
    print l[1] > l0_file
  }
  if (match(line, /imm_memtables=([0-9]+)/, i)) {
    print i[1] > imm_file
  }
}
' "$LOG_FILE" > "$TMP_PARSED"

BULK_ROWS="$(awk '$1 == "bulk" { c++ } END { print c + 0 }' "$TMP_PARSED")"
if [ "$BULK_ROWS" -gt 0 ]; then
  awk '$1 == "bulk" { print }' "$TMP_PARSED" > "$TMP_DATASET"
else
  awk '{ print }' "$TMP_PARSED" > "$TMP_DATASET"
fi

BATCHES="$(wc -l < "$TMP_DATASET" | tr -d ' ')"
BLOCKS="$(awk '{ s += $2 } END { printf "%.0f", s + 0 }' "$TMP_DATASET")"

if [ "$BATCHES" -eq 0 ]; then
  AVG_BATCH_SECONDS="0.000"
  P95_BATCH_SECONDS="0.000"
  P99_BATCH_SECONDS="0.000"
  AVG_COMMIT_MS="0.000"
  P95_COMMIT_MS="0.000"
  P99_COMMIT_MS="0.000"
else
  awk '{ print $3 }' "$TMP_DATASET" > "$TMP_SECS"
  awk '{ print $4 }' "$TMP_DATASET" > "$TMP_COMMITS"

  AVG_BATCH_SECONDS="$(awk '{ s += $1 } END { if (NR == 0) print "0.000"; else printf "%.3f", s / NR }' "$TMP_SECS")"
  AVG_COMMIT_MS="$(awk '{ s += $1 } END { if (NR == 0) print "0.000"; else printf "%.3f", s / NR }' "$TMP_COMMITS")"

  P95_INDEX=$(( (95 * BATCHES + 99) / 100 ))
  P99_INDEX=$(( (99 * BATCHES + 99) / 100 ))

  P95_BATCH_SECONDS="$(sort -n "$TMP_SECS" | awk -v idx="$P95_INDEX" 'NR == idx { printf "%.3f", $1; exit }')"
  P99_BATCH_SECONDS="$(sort -n "$TMP_SECS" | awk -v idx="$P99_INDEX" 'NR == idx { printf "%.3f", $1; exit }')"
  P95_COMMIT_MS="$(sort -n "$TMP_COMMITS" | awk -v idx="$P95_INDEX" 'NR == idx { printf "%.3f", $1; exit }')"
  P99_COMMIT_MS="$(sort -n "$TMP_COMMITS" | awk -v idx="$P99_INDEX" 'NR == idx { printf "%.3f", $1; exit }')"
fi

MAX_COMPACTION_PENDING_MB="$(if [ -s "$TMP_PENDING" ]; then sort -n "$TMP_PENDING" | tail -1; else echo 0; fi)"
MAX_L0_FILES="$(if [ -s "$TMP_L0" ]; then sort -n "$TMP_L0" | tail -1; else echo 0; fi)"
MAX_IMM_MEMTABLES="$(if [ -s "$TMP_IMM" ]; then sort -n "$TMP_IMM" | tail -1; else echo 0; fi)"

METRICS_FILE="${RUN_DIR}/metrics.env"
cat > "$METRICS_FILE" <<EOF
run_id=${RUN_ID}
status=${STATUS}
source_log=${LOG_FILE}
batches=${BATCHES}
blocks=${BLOCKS}
avg_batch_seconds=${AVG_BATCH_SECONDS}
p95_batch_seconds=${P95_BATCH_SECONDS}
p99_batch_seconds=${P99_BATCH_SECONDS}
avg_commit_ms=${AVG_COMMIT_MS}
p95_commit_ms=${P95_COMMIT_MS}
p99_commit_ms=${P99_COMMIT_MS}
max_compaction_pending_mb=${MAX_COMPACTION_PENDING_MB}
max_l0_files=${MAX_L0_FILES}
max_imm_memtables=${MAX_IMM_MEMTABLES}
generated_at_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)
EOF

read_metric() {
  local file="$1"
  local key="$2"
  awk -F= -v k="$key" '$1 == k { print $2; exit }' "$file"
}

delta_pct() {
  local current="$1"
  local baseline="$2"
  awk -v c="$current" -v b="$baseline" 'BEGIN {
    if (b == 0) {
      print "n/a"
    } else {
      printf "%.2f%%", ((c - b) / b) * 100.0
    }
  }'
}

REPORT_FILE="${RUN_DIR}/report.md"
BASELINE_FILE="${LATEST_DIR}/metrics.env"

{
  echo "# Bulk Sync Perf Report"
  echo ""
  echo "- Run ID: ${RUN_ID}"
  echo "- Status: ${STATUS}"
  echo "- Generated at (UTC): $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "- Source log: \`${LOG_FILE}\`"
  echo ""
  echo "## Current Metrics"
  echo ""
  echo "| Metric | Value |"
  echo "| --- | ---: |"
  echo "| batches | ${BATCHES} |"
  echo "| blocks | ${BLOCKS} |"
  echo "| avg_batch_seconds | ${AVG_BATCH_SECONDS} |"
  echo "| p95_batch_seconds | ${P95_BATCH_SECONDS} |"
  echo "| p99_batch_seconds | ${P99_BATCH_SECONDS} |"
  echo "| avg_commit_ms | ${AVG_COMMIT_MS} |"
  echo "| p95_commit_ms | ${P95_COMMIT_MS} |"
  echo "| p99_commit_ms | ${P99_COMMIT_MS} |"
  echo "| max_compaction_pending_mb | ${MAX_COMPACTION_PENDING_MB} |"
  echo "| max_l0_files | ${MAX_L0_FILES} |"
  echo "| max_imm_memtables | ${MAX_IMM_MEMTABLES} |"
  echo ""

  if [ -f "$BASELINE_FILE" ]; then
    BASELINE_RUN_ID="$(read_metric "$BASELINE_FILE" run_id)"
    echo "## Baseline Comparison"
    echo ""
    echo "- Baseline run: ${BASELINE_RUN_ID}"
    echo ""
    echo "| Metric | Current | Baseline | Delta |"
    echo "| --- | ---: | ---: | ---: |"
    for metric in \
      avg_batch_seconds \
      p95_batch_seconds \
      p99_batch_seconds \
      avg_commit_ms \
      p95_commit_ms \
      p99_commit_ms \
      max_compaction_pending_mb \
      max_l0_files \
      max_imm_memtables; do
      current_value="$(read_metric "$METRICS_FILE" "$metric")"
      baseline_value="$(read_metric "$BASELINE_FILE" "$metric")"
      echo "| ${metric} | ${current_value} | ${baseline_value} | $(delta_pct "$current_value" "$baseline_value") |"
    done
  else
    echo "## Baseline Comparison"
    echo ""
    echo "No previous baseline metrics found. This run can become the first baseline."
  fi
} > "$REPORT_FILE"

if [ "$STATUS" = "completed" ] && [ "$BATCHES" -gt 0 ]; then
  cp "$METRICS_FILE" "${LATEST_DIR}/metrics.env"
  cp "$REPORT_FILE" "${LATEST_DIR}/report.md"
fi

echo "metrics_file=${METRICS_FILE}"
echo "report_file=${REPORT_FILE}"
