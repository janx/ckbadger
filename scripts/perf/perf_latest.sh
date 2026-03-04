#!/usr/bin/env bash

set -euo pipefail

OUTPUT_ROOT="artifacts/perf/bulk-sync"

usage() {
  cat <<'EOF'
Usage: scripts/perf/perf_latest.sh [OPTIONS]

Options:
  --output-root DIR   Perf output root (default: artifacts/perf/bulk-sync)
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --output-root)
      OUTPUT_ROOT="$2"
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

if [ ! -d "$OUTPUT_ROOT" ]; then
  echo "perf output directory not found: $OUTPUT_ROOT" >&2
  exit 1
fi

LATEST_RUN_DIR="$(
  find "$OUTPUT_ROOT" -mindepth 1 -maxdepth 1 -type d ! -name latest \
    -exec test -f "{}/metrics.env" ';' -print \
    | sort | tail -1
)"

if [ -z "$LATEST_RUN_DIR" ]; then
  echo "no perf runs found under: $OUTPUT_ROOT" >&2
  exit 1
fi

METRICS_FILE="${LATEST_RUN_DIR}/metrics.env"
REPORT_FILE="${LATEST_RUN_DIR}/report.md"

read_metric() {
  local file="$1"
  local key="$2"
  awk -F= -v k="$key" '$1 == k { print $2; exit }' "$file"
}

RUN_ID="$(read_metric "$METRICS_FILE" run_id)"
STATUS="$(read_metric "$METRICS_FILE" status)"
GENERATED_AT="$(read_metric "$METRICS_FILE" generated_at_utc)"

echo "Bulk sync perf latest"
echo "run_id=${RUN_ID}"
echo "status=${STATUS}"
echo "generated_at_utc=${GENERATED_AT}"
echo "run_dir=${LATEST_RUN_DIR}"
echo "report=${REPORT_FILE}"
echo ""

if [ ! -f "$REPORT_FILE" ]; then
  echo "report file not found; showing current metrics only"
  for key in \
    batches \
    blocks \
    avg_batch_seconds \
    p95_batch_seconds \
    p99_batch_seconds \
    avg_commit_ms \
    p95_commit_ms \
    p99_commit_ms \
    max_compaction_pending_mb \
    max_l0_files \
    max_imm_memtables; do
    echo "${key}=$(read_metric "$METRICS_FILE" "$key")"
  done
  exit 0
fi

if grep -q '| Metric | Current | Baseline | Delta |' "$REPORT_FILE"; then
  BASELINE_RUN="$(awk -F': ' '/^- Baseline run:/ { print $2; exit }' "$REPORT_FILE")"
  echo "baseline_run=${BASELINE_RUN}"
  echo "key_deltas:"
  printf '%-28s %-12s %-12s %-12s\n' "metric" "current" "baseline" "delta"
  awk '
    BEGIN { in_table = 0 }
    /^\| Metric \| Current \| Baseline \| Delta \|$/ { in_table = 1; next }
    in_table && /^\| --- \|/ { next }
    in_table && /^\| [A-Za-z0-9_]+ \|/ {
      line = $0
      sub(/^\| /, "", line)
      sub(/ \|$/, "", line)
      n = split(line, cols, / \| /)
      if (n == 4) {
        printf "%-28s %-12s %-12s %-12s\n", cols[1], cols[2], cols[3], cols[4]
      }
      next
    }
    in_table && !/^\|/ { exit }
  ' "$REPORT_FILE"
else
  echo "baseline comparison not available in latest report"
fi
