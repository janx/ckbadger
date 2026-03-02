#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

expect_contains() {
  local haystack="$1"
  local needle="$2"
  if [[ "$haystack" != *"$needle"* ]]; then
    echo "expected output to contain: $needle" >&2
    exit 1
  fi
}

expect_not_contains() {
  local haystack="$1"
  local needle="$2"
  if [[ "$haystack" == *"$needle"* ]]; then
    echo "expected output not to contain: $needle" >&2
    exit 1
  fi
}

TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMP_DIR"' EXIT

mkdir -p "$TEMP_DIR/bin"
TEST_LOG="$TEMP_DIR/cmd.log"

cat > "$TEMP_DIR/bin/cargo" <<'SCRIPT'
#!/usr/bin/env bash
set -euo pipefail
echo "cargo $*" >> "$CKBADGER_TUI_TEST_LOG"
exit 0
SCRIPT

cat > "$TEMP_DIR/bin/docker" <<'SCRIPT'
#!/usr/bin/env bash
set -euo pipefail
echo "docker $*" >> "$CKBADGER_TUI_TEST_LOG"

if [[ "${1:-}" == "ps" && "${2:-}" == "--format" ]]; then
  echo "${MOCK_DOCKER_PS_OUTPUT:-}"
  exit 0
fi

if [[ "${1:-}" == "image" && "${2:-}" == "inspect" ]]; then
  exit "${MOCK_DOCKER_IMAGE_INSPECT_EXIT:-0}"
fi

exit 0
SCRIPT

chmod +x "$TEMP_DIR/bin/cargo" "$TEMP_DIR/bin/docker"

PATH="$TEMP_DIR/bin:$PATH"
export PATH
export CKBADGER_TUI_TEST_LOG="$TEST_LOG"

# Case 1: local RocksDB exists -> use local cargo run
: > "$TEST_LOG"
LOCAL_DB="$TEMP_DIR/local-db"
mkdir -p "$LOCAL_DB"
touch "$LOCAL_DB/CURRENT"
case1_out="$({ CKBADGER_DOMAIN_DATA_PATH="$LOCAL_DB" TUI_ARGS="--refresh-ms 500" ./scripts/run_tui.sh; } 2>&1)"
case1_log="$(cat "$TEST_LOG")"
expect_contains "$case1_out" "Using local RocksDB"
expect_contains "$case1_log" "cargo run -p ckbadger-tui -- --refresh-ms 500"
expect_not_contains "$case1_log" "docker run"

# Case 2: local missing + docker indexer running -> use docker mode
: > "$TEST_LOG"
case2_out="$({ CKBADGER_DOMAIN_DATA_PATH="$TEMP_DIR/missing-db" MOCK_DOCKER_PS_OUTPUT="ckbadger-indexer" MOCK_DOCKER_IMAGE_INSPECT_EXIT=1 TUI_ARGS="--refresh-ms 200" ./scripts/run_tui.sh; } 2>&1)"
case2_log="$(cat "$TEST_LOG")"
expect_contains "$case2_out" "using Docker indexer volume"
expect_contains "$case2_log" "docker image inspect ckbadger-tui"
expect_contains "$case2_log" "docker build -f docker/Dockerfile.tui -t ckbadger-tui ."
expect_contains "$case2_log" "docker run --rm"
expect_contains "$case2_log" "--volumes-from ckbadger-indexer"
expect_contains "$case2_log" "-e CKBADGER_DOMAIN_DATA_PATH=/data/ckbadger-store"
expect_not_contains "$case2_log" "cargo run"

# Case 3: local missing + docker indexer absent -> fail fast
: > "$TEST_LOG"
set +e
case3_out="$({ CKBADGER_DOMAIN_DATA_PATH="$TEMP_DIR/missing-db" MOCK_DOCKER_PS_OUTPUT="" ./scripts/run_tui.sh; } 2>&1)"
case3_rc=$?
set -e
if [[ $case3_rc -eq 0 ]]; then
  echo "expected missing-db case to fail" >&2
  exit 1
fi
expect_contains "$case3_out" "No readable local RocksDB"

echo "run_tui script coverage passed"
