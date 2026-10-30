#!/usr/bin/env bash
# Reclaim cargo build output in the main checkout and every git worktree.
#
#   scripts/sweep-targets.sh           dry run: report what would be removed
#   scripts/sweep-targets.sh --apply   remove it
#
# 1. A worktree with no uncommitted changes whose HEAD is already in main loses
#    its whole target/: its work is merged, nothing there will be built again.
# 2. Every other target/ (the main checkout's included) keeps only artifacts
#    built by SWEEP_TOOLCHAINS (default: the checkout's active toolchain) within
#    SWEEP_DAYS (default: 14), via cargo-sweep, and drops incremental caches
#    untouched for as long.
#
# /home is mounted noatime, so cargo-sweep's "last used" is really "built":
# an artifact still in use but not rebuilt for SWEEP_DAYS is removed and gets
# rebuilt by the next build.
#
# A checkout is skipped while cargo works in it: cargo holds
# <profile>/.cargo-lock only while it compiles, not while it runs test
# binaries, so a cargo process whose working tree is the checkout counts too.
# That check reads /proc, so run this outside a PID-namespace sandbox.
#
# Environment: SWEEP_DAYS, SWEEP_TOOLCHAINS, SWEEP_MAIN_REF (default: main).

set -euo pipefail

apply=false
case "${1:-}" in
  --apply) apply=true ;;
  "") ;;
  *)
    echo "usage: $0 [--apply]" >&2
    exit 2
    ;;
esac

days="${SWEEP_DAYS:-14}"
main_ref="${SWEEP_MAIN_REF:-main}"

if ! command -v cargo-sweep >/dev/null; then
  echo "ERROR: cargo-sweep is not installed: cargo install cargo-sweep --locked" >&2
  exit 1
fi

main_checkout="$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")"
toolchains="${SWEEP_TOOLCHAINS:-$(cd "$main_checkout" && rustup show active-toolchain | cut -d' ' -f1)}"
# cargo-sweep wants full `rustup toolchain list` names and exits 0 on others.
IFS=, read -ra wanted <<<"$toolchains"
for toolchain in "${wanted[@]}"; do
  rustup toolchain list | cut -d' ' -f1 | grep -qxF "$toolchain" || {
    echo "ERROR: SWEEP_TOOLCHAINS names $toolchain, not a \`rustup toolchain list\` entry" >&2
    exit 1
  }
done
git -C "$main_checkout" rev-parse --verify --quiet "$main_ref^{commit}" >/dev/null || {
  echo "ERROR: $main_ref is not a commit in $main_checkout" >&2
  exit 1
}

sweep_flags=()
$apply || sweep_flags+=(--dry-run)

checkout_busy() {
  local checkout="$1" lock pid cwd
  while IFS= read -r -d '' lock; do
    flock --nonblock "$lock" true || return 0
  done < <(find "$checkout/target" -maxdepth 3 -name .cargo-lock -print0)
  for pid in $(pgrep -x cargo); do
    cwd="$(readlink "/proc/$pid/cwd")" || continue
    [[ "$(git -C "$cwd" rev-parse --show-toplevel 2>/dev/null)" == "$checkout" ]] && return 0
  done
  return 1
}

# cargo-sweep logs its failures and still exits 0.
cargo_sweep() {
  local log
  log="$(cargo sweep "${sweep_flags[@]}" "$@" 2>&1)" || {
    echo "$log" >&2
    return 1
  }
  grep -v '^\[INFO\] Using specified installed toolchains' <<<"$log" || true
  if grep -q '^\[ERROR\]' <<<"$log"; then
    echo "ERROR: cargo sweep $* failed" >&2
    return 1
  fi
}

sweep_target() {
  local checkout="$1" target="$1/target" cache
  cargo_sweep --toolchains "$toolchains" "$checkout"
  cargo_sweep --time "$days" "$checkout"
  while IFS= read -r -d '' cache; do
    echo "  stale incremental cache: ${cache#"$main_checkout"/}"
    if $apply; then rm -rf -- "$cache"; fi
  done < <(find "$target" \( -name deps -o -name build -o -name .fingerprint \) -prune \
    -o -type d -name incremental -prune -print0 |
    xargs -0 -r -I{} find {} -mindepth 1 -maxdepth 1 -type d -mtime "+$days" -print0)
}

while IFS= read -r line; do
  [[ "$line" == "worktree "* ]] || continue
  checkout="${line#worktree }"
  target="$checkout/target"
  name="${checkout#"$main_checkout"}"
  name="${name#/}"
  name="${name:-main checkout}"
  [[ -d "$target" ]] || continue

  if checkout_busy "$checkout"; then
    echo "== $name: cargo is working in it, skipped"
    continue
  fi

  if [[ "$checkout" != "$main_checkout" ]] &&
    [[ -z "$(git -C "$checkout" status --porcelain)" ]] &&
    git -C "$checkout" merge-base --is-ancestor HEAD "$main_ref"; then
    echo "== $name: merged into $main_ref, remove target/ ($(du -sh "$target" | cut -f1))"
    if $apply; then rm -rf -- "$target"; fi
    continue
  fi

  echo "== $name: sweep target/"
  sweep_target "$checkout"
done < <(git -C "$main_checkout" worktree list --porcelain)

$apply || echo "Dry run; rerun with --apply to remove."
