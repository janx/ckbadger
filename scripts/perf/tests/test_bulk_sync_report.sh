#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
OUTPUT_ROOT="$(mktemp -d)"
trap 'rm -rf "$OUTPUT_ROOT"' EXIT

FAST_LOG="${ROOT_DIR}/scripts/perf/tests/fixtures/bulk_sync_sample_fast.log"
SLOW_LOG="${ROOT_DIR}/scripts/perf/tests/fixtures/bulk_sync_sample_slow.log"

bash "${ROOT_DIR}/scripts/perf/bulk_sync_report.sh" \
  --log "$FAST_LOG" \
  --output-root "$OUTPUT_ROOT" \
  --run-id fast-run \
  --status completed >/dev/null

FAST_METRICS="${OUTPUT_ROOT}/fast-run/metrics.env"
if [ ! -f "$FAST_METRICS" ]; then
  echo "missing metrics file for fast run" >&2
  exit 1
fi

grep -q '^batches=3$' "$FAST_METRICS"
grep -q '^blocks=300$' "$FAST_METRICS"
grep -q '^avg_batch_seconds=2.000$' "$FAST_METRICS"
grep -q '^p95_batch_seconds=3.000$' "$FAST_METRICS"
grep -q '^p99_batch_seconds=3.000$' "$FAST_METRICS"
grep -q '^avg_commit_ms=200.000$' "$FAST_METRICS"
grep -q '^max_compaction_pending_mb=150$' "$FAST_METRICS"
grep -q '^max_l0_files=20$' "$FAST_METRICS"
grep -q '^max_imm_memtables=5$' "$FAST_METRICS"

bash "${ROOT_DIR}/scripts/perf/bulk_sync_report.sh" \
  --log "$SLOW_LOG" \
  --output-root "$OUTPUT_ROOT" \
  --run-id slow-run \
  --status completed >/dev/null

SLOW_REPORT="${OUTPUT_ROOT}/slow-run/report.md"
if [ ! -f "$SLOW_REPORT" ]; then
  echo "missing report file for slow run" >&2
  exit 1
fi

grep -q 'Baseline run: fast-run' "$SLOW_REPORT"
grep -q '| avg_batch_seconds | 4.000 | 2.000 | 100.00% |' "$SLOW_REPORT"
grep -q '| max_l0_files | 30 | 20 | 50.00% |' "$SLOW_REPORT"

echo "test_bulk_sync_report: ok"
