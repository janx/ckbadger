#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  scripts/run_tx_debugger_matrix.sh <tx_hash> [base_url]

Arguments:
  tx_hash   Transaction hash (0x-prefixed or non-prefixed)
  base_url  Frontend base URL (default: http://localhost:3000)

Environment variables:
  SCRIPT_GROUP_TYPES   Space-separated list, default: "lock type"
  CELL_TYPES           Space-separated list, default: "input output"
  CONTINUE_ON_ERROR    "1" to continue after a failed combination (default: 0)

Examples:
  scripts/run_tx_debugger_matrix.sh 0xabc...def
  SCRIPT_GROUP_TYPES="lock" CELL_TYPES="input" scripts/run_tx_debugger_matrix.sh 0xabc...def
  CONTINUE_ON_ERROR=1 scripts/run_tx_debugger_matrix.sh 0xabc...def http://localhost:3000
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

if [[ $# -lt 1 || $# -gt 2 ]]; then
  usage
  exit 2
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "error: jq is required but not found in PATH" >&2
  exit 127
fi

if ! command -v ckb-debugger >/dev/null 2>&1; then
  echo "error: ckb-debugger is required but not found in PATH" >&2
  exit 127
fi

TX_HASH="$1"
BASE_URL="${2:-http://localhost:3000}"
BASE_URL="${BASE_URL%/}"
SCRIPT_GROUP_TYPES="${SCRIPT_GROUP_TYPES:-lock type}"
CELL_TYPES="${CELL_TYPES:-input output}"
CONTINUE_ON_ERROR="${CONTINUE_ON_ERROR:-0}"

if [[ "${TX_HASH:0:2}" != "0x" ]]; then
  TX_HASH="0x${TX_HASH}"
fi

RAW_URL="${BASE_URL}/tx/${TX_HASH}.raw?profile=debugger"
TMP_DIR="$(mktemp -d)"
RAW_JSON="${TMP_DIR}/tx-debugger-raw.json"
MOCK_TX_JSON="${TMP_DIR}/mock-tx.json"
trap 'rm -rf "${TMP_DIR}"' EXIT

echo "[1/4] Fetch raw debugger payload: ${RAW_URL}"
curl -fsSL "${RAW_URL}" > "${RAW_JSON}"

if jq -e '.error != null' "${RAW_JSON}" >/dev/null; then
  echo "error: raw endpoint returned an error payload" >&2
  jq '.error' "${RAW_JSON}" >&2
  exit 1
fi

echo "[2/4] Extract mock transaction"
jq -e '.data.txDebugger.mockTransaction' "${RAW_JSON}" > "${MOCK_TX_JSON}" >/dev/null
jq -e '.data.txDebugger.mockTransaction' "${RAW_JSON}" > "${MOCK_TX_JSON}"

INPUT_COUNT="$(jq -r '.tx.inputs | length' "${MOCK_TX_JSON}")"
OUTPUT_COUNT="$(jq -r '.tx.outputs | length' "${MOCK_TX_JSON}")"

echo "[3/4] Matrix preparation"
echo "  tx_hash=${TX_HASH}"
echo "  input_count=${INPUT_COUNT}"
echo "  output_count=${OUTPUT_COUNT}"
echo "  script_group_types=${SCRIPT_GROUP_TYPES}"
echo "  cell_types=${CELL_TYPES}"

run_debugger() {
  local script_group_type="$1"
  local cell_type="$2"
  local cell_index="$3"

  echo "[4/4] Run: script_group_type=${script_group_type} cell_type=${cell_type} cell_index=${cell_index}"
  ckb-debugger \
    --tx-file "${MOCK_TX_JSON}" \
    --cell-index "${cell_index}" \
    --cell-type "${cell_type}" \
    --script-group-type "${script_group_type}"
}

TOTAL=0
FAILED=0

for script_group_type in ${SCRIPT_GROUP_TYPES}; do
  for cell_type in ${CELL_TYPES}; do
    case "${cell_type}" in
      input)
        max_count="${INPUT_COUNT}"
        ;;
      output)
        max_count="${OUTPUT_COUNT}"
        ;;
      *)
        echo "error: unsupported CELL_TYPES value '${cell_type}' (allowed: input/output)" >&2
        exit 2
        ;;
    esac

    for ((idx = 0; idx < max_count; idx++)); do
      TOTAL=$((TOTAL + 1))
      if ! run_debugger "${script_group_type}" "${cell_type}" "${idx}"; then
        FAILED=$((FAILED + 1))
        echo "error: debugger failed at script_group_type=${script_group_type} cell_type=${cell_type} cell_index=${idx}" >&2
        if [[ "${CONTINUE_ON_ERROR}" != "1" ]]; then
          exit 1
        fi
      fi
    done
  done
done

echo "completed: total_runs=${TOTAL} failed_runs=${FAILED}"
if [[ "${FAILED}" -gt 0 ]]; then
  exit 1
fi
