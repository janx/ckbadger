#!/usr/bin/env bash
set -euo pipefail

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

external_output="$(make -n up CKB_NODE_MODE=external)"
expect_contains "$external_output" "docker compose up -d redis indexer api frontend"
expect_not_contains "$external_output" "--build"
expect_not_contains "$external_output" "--force-recreate"

internal_output="$(make -n up CKB_NODE_MODE=internal)"
expect_contains "$internal_output" "docker compose --profile internal up -d redis ckb-node indexer api frontend"
expect_not_contains "$internal_output" "--build"
expect_not_contains "$internal_output" "--force-recreate"

echo "make up command coverage passed"
