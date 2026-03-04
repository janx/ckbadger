#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
OUTPUT_ROOT="$(mktemp -d)"
trap 'rm -rf "$OUTPUT_ROOT"' EXIT

mkdir -p "${OUTPUT_ROOT}/20260304T120000Z" "${OUTPUT_ROOT}/20260305T120000Z"

cat > "${OUTPUT_ROOT}/20260304T120000Z/metrics.env" <<'EOF'
run_id=20260304T120000Z
status=completed
generated_at_utc=2026-03-04T12:00:00Z
EOF

cat > "${OUTPUT_ROOT}/20260304T120000Z/report.md" <<'EOF'
# Bulk Sync Perf Report

- Run ID: 20260304T120000Z
EOF

cat > "${OUTPUT_ROOT}/20260305T120000Z/metrics.env" <<'EOF'
run_id=20260305T120000Z
status=completed
generated_at_utc=2026-03-05T12:00:00Z
EOF

cat > "${OUTPUT_ROOT}/20260305T120000Z/report.md" <<'EOF'
# Bulk Sync Perf Report

## Baseline Comparison

- Baseline run: 20260304T120000Z

| Metric | Current | Baseline | Delta |
| --- | ---: | ---: | ---: |
| avg_batch_seconds | 4.000 | 2.000 | 100.00% |
| p95_batch_seconds | 8.000 | 6.000 | 33.33% |
EOF

OUT="$(
  bash "${ROOT_DIR}/scripts/perf/perf_latest.sh" --output-root "$OUTPUT_ROOT"
)"

echo "$OUT" | grep -q '^run_id=20260305T120000Z$'
echo "$OUT" | grep -q '^baseline_run=20260304T120000Z$'
echo "$OUT" | grep -q '^avg_batch_seconds'
echo "$OUT" | grep -q '100.00%'

echo "test_perf_latest: ok"
