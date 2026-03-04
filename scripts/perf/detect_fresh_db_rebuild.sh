#!/usr/bin/env bash

set -euo pipefail

COMPOSE_PROJECT="${COMPOSE_PROJECT:-$(basename "$(pwd)")}"
PROBE_IMAGE="${PERF_DOCKER_PROBE_IMAGE:-redis:7-alpine}"
VOLUME_NAME=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --compose-project)
      COMPOSE_PROJECT="$2"
      shift 2
      ;;
    --volume)
      VOLUME_NAME="$2"
      shift 2
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

if ! command -v docker >/dev/null 2>&1; then
  echo "docker is not available" >&2
  exit 2
fi

if [ -z "$VOLUME_NAME" ]; then
  VOLUME_NAME="$(docker volume ls -q \
    --filter "label=com.docker.compose.project=${COMPOSE_PROJECT}" \
    --filter "label=com.docker.compose.volume=ckbadger-data" | head -n 1)"
fi

if [ -z "$VOLUME_NAME" ]; then
  CANDIDATE="${COMPOSE_PROJECT}_ckbadger-data"
  if docker volume inspect "$CANDIDATE" >/dev/null 2>&1; then
    VOLUME_NAME="$CANDIDATE"
  fi
fi

if [ -z "$VOLUME_NAME" ]; then
  # No existing volume: this is a fresh DB rebuild context.
  exit 0
fi

if ! docker image inspect "$PROBE_IMAGE" >/dev/null 2>&1; then
  echo "probe image unavailable: $PROBE_IMAGE" >&2
  exit 2
fi

set +e
docker run --rm -v "${VOLUME_NAME}:/data:ro" "$PROBE_IMAGE" sh -ec '
for dir in /data/ckbadger-store /data/ckbadger-store-append-only; do
  if [ -d "$dir" ] && find "$dir" -mindepth 1 -print -quit | grep -q .; then
    exit 1
  fi
done
exit 0
'
RC=$?
set -e

if [ "$RC" -eq 0 ]; then
  exit 0
fi
if [ "$RC" -eq 1 ]; then
  exit 1
fi

echo "failed to inspect volume state: ${VOLUME_NAME}" >&2
exit 2
