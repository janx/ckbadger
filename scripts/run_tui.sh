#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

DOMAIN_DATA_PATH="${CKBADGER_DOMAIN_DATA_PATH:-./data/ckbadger-store}"
INDEXER_CONTAINER="${CKBADGER_TUI_INDEXER_CONTAINER:-ckbadger-indexer}"
TUI_IMAGE="${CKBADGER_TUI_IMAGE:-ckbadger-tui}"
DOCKER_DOMAIN_DATA_PATH="${CKBADGER_TUI_DOCKER_DOMAIN_DATA_PATH:-/data/ckbadger-store}"
SECONDARY_DATA_PATH="${CKBADGER_TUI_SECONDARY_DATA_PATH:-/tmp/ckbadger-store-tui-secondary}"
API_URL_VALUE="${API_URL:-http://localhost:3001/api/v1}"
TUI_ARGS_RAW="${TUI_ARGS:-}"

extra_args=()
if [[ -n "$TUI_ARGS_RAW" ]]; then
  # shellcheck disable=SC2206
  extra_args=($TUI_ARGS_RAW)
fi

run_local_tui() {
  local -a cmd=(cargo run -p ckbadger-tui)
  if ((${#extra_args[@]} > 0)); then
    cmd+=(--)
    cmd+=("${extra_args[@]}")
  fi
  "${cmd[@]}"
}

has_local_rocksdb() {
  [[ -f "$DOMAIN_DATA_PATH/CURRENT" ]]
}

docker_indexer_running() {
  command -v docker >/dev/null 2>&1 || return 1
  docker ps --format '{{.Names}}' | grep -Fxq "$INDEXER_CONTAINER"
}

ensure_tui_image() {
  if ! docker image inspect "$TUI_IMAGE" >/dev/null 2>&1; then
    echo "Local RocksDB missing; building Docker TUI image '$TUI_IMAGE'..."
    docker build -f docker/Dockerfile.tui -t "$TUI_IMAGE" .
  fi
}

run_docker_tui() {
  local -a cmd=(docker run --rm)
  if [[ -t 0 && -t 1 ]]; then
    cmd+=(-it)
  fi

  cmd+=(
    --network host
    --volumes-from "$INDEXER_CONTAINER"
    -e "CKBADGER_DOMAIN_DATA_PATH=$DOCKER_DOMAIN_DATA_PATH"
    -e "CKBADGER_TUI_SECONDARY_DATA_PATH=$SECONDARY_DATA_PATH"
    -e "API_URL=$API_URL_VALUE"
  )

  if [[ -n "${REDIS_URL:-}" ]]; then
    cmd+=(-e "REDIS_URL=$REDIS_URL")
  fi

  cmd+=("$TUI_IMAGE")
  if ((${#extra_args[@]} > 0)); then
    cmd+=("${extra_args[@]}")
  fi

  "${cmd[@]}"
}

if has_local_rocksdb; then
  echo "Using local RocksDB: $DOMAIN_DATA_PATH"
  run_local_tui
  exit 0
fi

if docker_indexer_running; then
  echo "Local RocksDB not found at '$DOMAIN_DATA_PATH'; using Docker indexer volume via '$INDEXER_CONTAINER'."
  ensure_tui_image
  run_docker_tui
  exit 0
fi

cat <<MSG >&2
Error: No readable local RocksDB found at '$DOMAIN_DATA_PATH/CURRENT', and Docker indexer container '$INDEXER_CONTAINER' is not running.
Start services with 'make up', or set CKBADGER_DOMAIN_DATA_PATH to a local RocksDB path.
MSG
exit 1
