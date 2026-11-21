#!/usr/bin/env bash
set -euo pipefail

image="${1:?usage: test_docker_shutdown.sh IMAGE}"
workdir="$(mktemp -d)"
container=""
cleanup() {
  if [[ -n "$container" ]]; then
    docker rm -f "$container" >/dev/null || true
  fi
  rm -rf "$workdir"
}
trap cleanup EXIT

# Use a real supervised service without needing a CKB node or chain data.
printf '[frontend]\nport = 0\n' > "$workdir/config.toml"
container="$(docker run --detach --init --network none --stop-timeout 20 \
  --user "$(id -u):$(id -g)" --mount "type=bind,src=$workdir,dst=/work" \
  "$image" run --only frontend-server)"

supervisor_ready() {
  local pid
  [[ -S "$workdir/run/indexer.sock" && -s "$workdir/run/supervisor.pid" ]] || return 1
  pid=$(<"$workdir/run/supervisor.pid") || return 1
  [[ "$pid" =~ ^[1-9][0-9]*$ ]] || return 1

  # The IPC socket can appear before Tokio registers SIGINT. Check the
  # supervisor's caught-signal mask inside the container's PID namespace.
  # SIGINT (signal 2) is bit 1 of the least-significant hex digit.
  docker exec "$container" awk '
    $1 == "SigCgt:" { caught = (tolower($2) ~ /[2367abef]$/) }
    END { exit !caught }
  ' "/proc/$pid/status" 2>/dev/null
}

ready=false
for ((attempt = 0; attempt < 100; attempt++)); do
  if supervisor_ready; then
    ready=true
    break
  fi
  sleep 0.1
done
if [[ "$ready" != true ]]; then
  docker logs "$container"
  echo 'CKBadger supervisor did not become ready' >&2
  exit 1
fi

docker stop "$container" >/dev/null
if [[ "$(docker inspect --format '{{.State.ExitCode}}' "$container")" != 0 ||
      -e "$workdir/run/supervisor.pid" || -e "$workdir/run/indexer.sock" ]]; then
  docker logs "$container"
  echo 'CKBadger did not shut down cleanly' >&2
  exit 1
fi
