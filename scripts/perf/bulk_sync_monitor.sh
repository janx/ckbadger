#!/usr/bin/env bash

set -euo pipefail

OUTPUT_ROOT="artifacts/perf/bulk-sync"
RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
CONTAINER="ckbadger-indexer"
MAX_SECONDS=172800
POLL_SECONDS=20
COMPOSE_PROJECT="${COMPOSE_PROJECT:-$(basename "$(pwd)")}"

usage() {
  cat <<'EOF'
Usage: scripts/perf/bulk_sync_monitor.sh [OPTIONS]

Options:
  --output-root DIR     Output root directory (default: artifacts/perf/bulk-sync)
  --run-id ID           Run ID (default: current UTC timestamp)
  --container NAME      Docker container name (default: ckbadger-indexer)
  --max-seconds N       Max monitor duration in seconds (default: 172800)
  --poll-seconds N      Poll interval seconds (default: 20)
  --compose-project P   Compose project name for metadata
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --output-root)
      OUTPUT_ROOT="$2"
      shift 2
      ;;
    --run-id)
      RUN_ID="$2"
      shift 2
      ;;
    --container)
      CONTAINER="$2"
      shift 2
      ;;
    --max-seconds)
      MAX_SECONDS="$2"
      shift 2
      ;;
    --poll-seconds)
      POLL_SECONDS="$2"
      shift 2
      ;;
    --compose-project)
      COMPOSE_PROJECT="$2"
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

if ! command -v docker >/dev/null 2>&1; then
  echo "docker is not available" >&2
  exit 1
fi

RUN_DIR="${OUTPUT_ROOT}/${RUN_ID}"
LOG_FILE="${RUN_DIR}/indexer.log"
STATUS_FILE="${RUN_DIR}/status.env"
META_FILE="${RUN_DIR}/metadata.env"
mkdir -p "$RUN_DIR"

{
  echo "run_id=${RUN_ID}"
  echo "container=${CONTAINER}"
  echo "compose_project=${COMPOSE_PROJECT}"
  echo "started_at_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "git_sha=$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
} > "$META_FILE"

READY_DEADLINE=$(( $(date +%s) + 180 ))
while ! docker ps --format '{{.Names}}' | grep -qx "$CONTAINER"; do
  if [ "$(date +%s)" -ge "$READY_DEADLINE" ]; then
    echo "indexer container did not become ready: ${CONTAINER}" >&2
    echo "status=container_not_running" > "$STATUS_FILE"
    exit 1
  fi
  sleep 2
done

START_SINCE="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
docker logs --since "$START_SINCE" -f "$CONTAINER" 2>&1 \
  | sed -u 's/\x1B\[[0-9;]*[A-Za-z]//g' > "$LOG_FILE" &
LOG_PID=$!

cleanup() {
  if kill -0 "$LOG_PID" >/dev/null 2>&1; then
    kill "$LOG_PID" >/dev/null 2>&1 || true
    wait "$LOG_PID" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

STATUS="partial_timeout"
DEADLINE=$(( $(date +%s) + MAX_SECONDS ))

while [ "$(date +%s)" -lt "$DEADLINE" ]; do
  if grep -q "Bulk sync completed" "$LOG_FILE"; then
    STATUS="completed"
    break
  fi
  if ! docker ps --format '{{.Names}}' | grep -qx "$CONTAINER"; then
    STATUS="container_stopped"
    break
  fi
  sleep "$POLL_SECONDS"
done

cleanup
trap - EXIT

scripts/perf/bulk_sync_report.sh \
  --log "$LOG_FILE" \
  --output-root "$OUTPUT_ROOT" \
  --run-id "$RUN_ID" \
  --status "$STATUS" >/dev/null

{
  echo "status=${STATUS}"
  echo "finished_at_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "log_file=${LOG_FILE}"
} > "$STATUS_FILE"

echo "bulk_sync_monitor_status=${STATUS}"
echo "run_dir=${RUN_DIR}"
