#!/usr/bin/env bash
# Regression test for scripts/sweep-targets.sh:
#
#   scripts/test_sweep_targets.sh
#
# Builds a throwaway repository under $TMPDIR (a minimal cargo package, since
# cargo-sweep finds target/ through `cargo metadata`) with a main branch and
# four linked worktrees, each with a target/ holding one file:
#
#   fresh-branch  `git worktree add -b` from main, nothing done since
#   fresh-detach  `git worktree add --detach` at an older main commit
#   merged        a branch with its own commit, merged into main with --no-ff
#   no-reflog     like merged, but made with core.logAllRefUpdates=false, so
#                 no reflog shows that HEAD moved since creation
#
# All four are clean and in main. Only merged's target/ may be removed whole;
# the others are swept by age, as is the main checkout, and keep their
# just-written file. The script runs from inside the throwaway repository,
# which makes it the main checkout, so this checkout's build output is never
# touched. Needs cargo-sweep and rustup, like the script itself.

set -euo pipefail

sweep="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/sweep-targets.sh"

# Nothing from the caller's environment may point git, cargo or the script
# elsewhere: no outer repository, user git config or target-dir override.
unset GIT_DIR GIT_WORK_TREE GIT_COMMON_DIR GIT_INDEX_FILE GIT_OBJECT_DIRECTORY \
  CARGO_TARGET_DIR CARGO_BUILD_TARGET_DIR SWEEP_DAYS SWEEP_TOOLCHAINS SWEEP_MAIN_REF
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME=sweep-test GIT_AUTHOR_EMAIL=sweep-test@localhost
export GIT_COMMITTER_NAME=sweep-test GIT_COMMITTER_EMAIL=sweep-test@localhost

tmp="$(mktemp -d "${TMPDIR:-/tmp}/test_sweep_targets.XXXXXX")"
trap 'rm -rf -- "$tmp"' EXIT
tmp="$(cd "$tmp" && pwd -P)"
repo="$tmp/repo"
wt="$repo/.worktrees"

# --- fixture -----------------------------------------------------------------

git init -q -b main "$repo"
mkdir "$repo/src"
cat >"$repo/Cargo.toml" <<'EOF'
[package]
name = "sweep-fixture"
version = "0.1.0"
edition = "2021"

[workspace]
EOF
echo 'fn main() {}' >"$repo/src/main.rs"
printf '%s\n' /target/ /.worktrees/ /Cargo.lock >"$repo/.gitignore"
git -C "$repo" add -A
git -C "$repo" commit -qm 'fixture: cargo package'
older="$(git -C "$repo" rev-parse HEAD)"
git -C "$repo" commit -q --allow-empty -m 'fixture: second'
git -C "$repo" commit -q --allow-empty -m 'fixture: third'

git -C "$repo" worktree add -q -b merged-work "$wt/merged" main
echo merged >"$wt/merged/merged.txt"
git -C "$wt/merged" add merged.txt
git -C "$wt/merged" commit -qm 'merged: own commit'
git -C "$repo" merge -q --no-ff --no-edit -m 'Merge merged-work' merged-work

# Without reflogs: with no HEAD reflog, `git reflog show HEAD` falls back to
# the branch's, so neither may be written.
git -C "$repo" -c core.logAllRefUpdates=false worktree add -q -b no-reflog-work "$wt/no-reflog" main
echo no-reflog >"$wt/no-reflog/no-reflog.txt"
git -C "$wt/no-reflog" add no-reflog.txt
git -C "$wt/no-reflog" -c core.logAllRefUpdates=false commit -qm 'no-reflog: own commit'
git -C "$repo" merge -q --no-ff --no-edit -m 'Merge no-reflog-work' no-reflog-work

git -C "$repo" worktree add -q -b fresh-work "$wt/fresh-branch" main
git -C "$repo" worktree add -q --detach "$wt/fresh-detach" "$older"

checkouts=("$repo" "$wt/merged" "$wt/no-reflog" "$wt/fresh-branch" "$wt/fresh-detach")
for checkout in "${checkouts[@]}"; do
  mkdir "$checkout/target"
  echo built >"$checkout/target/artifact"
done

# --- checks ------------------------------------------------------------------

failures=0
ok() { echo "ok - $1"; }
not_ok() {
  echo "not ok - $1"
  failures=$((failures + 1))
}
check() {
  local what="$1"
  shift
  if "$@"; then ok "$what"; else not_ok "$what"; fi
}
has_line() { grep -qxF -- "$2" <<<"$1"; }
has_line_starting() {
  local line
  while IFS= read -r line; do
    [[ "$line" == "$2"* ]] && return 0
  done <<<"$1"
  return 1
}
removed() { [[ ! -e "$1" ]]; }

# Fixture preconditions: every worktree meets the old rule 1 (clean, HEAD in
# main), and the script would see this repository as its main checkout.
for checkout in "${checkouts[@]:1}"; do
  name="${checkout#"$repo"/}"
  check "fixture: $name has no uncommitted changes" \
    test -z "$(git -C "$checkout" status --porcelain)"
  check "fixture: $name HEAD is in main" \
    git -C "$checkout" merge-base --is-ancestor HEAD main
done
check "fixture: .worktrees/no-reflog has an empty HEAD reflog" \
  test -z "$(git -C "$wt/no-reflog" reflog show --format=%H HEAD)"
check "fixture: the script's main checkout is the throwaway repository" \
  test "$(cd "$repo" && git rev-parse --path-format=absolute --git-common-dir)" = "$repo/.git"
if ((failures)); then
  echo "FAIL: fixture is not what the test needs" >&2
  exit 1
fi

run_sweep() { (cd "$repo" && "$sweep" "$@") 2>&1; }

if ! dry="$(run_sweep)"; then
  printf '%s\n' "$dry"
  echo "FAIL: dry run exited non-zero" >&2
  exit 1
fi
printf '%s\n' "--- dry run" "$dry" "---"
check "dry run: fresh-branch is swept, not removed" \
  has_line "$dry" "== .worktrees/fresh-branch: sweep target/"
check "dry run: fresh-detach is swept, not removed" \
  has_line "$dry" "== .worktrees/fresh-detach: sweep target/"
check "dry run: no-reflog is swept, not removed" \
  has_line "$dry" "== .worktrees/no-reflog: sweep target/"
check "dry run: merged loses its target/" \
  has_line_starting "$dry" "== .worktrees/merged: merged into main, remove target/ ("
check "dry run: main checkout is swept" \
  has_line "$dry" "== main checkout: sweep target/"
for checkout in "${checkouts[@]}"; do
  check "dry run: ${checkout#"$tmp"/}/target/artifact still there" \
    test -f "$checkout/target/artifact"
done

if ! applied="$(run_sweep --apply)"; then
  printf '%s\n' "$applied"
  echo "FAIL: --apply exited non-zero" >&2
  exit 1
fi
printf '%s\n' "--- --apply" "$applied" "---"
check "--apply: fresh-branch keeps target/artifact" test -f "$wt/fresh-branch/target/artifact"
check "--apply: fresh-detach keeps target/artifact" test -f "$wt/fresh-detach/target/artifact"
check "--apply: no-reflog keeps target/artifact" test -f "$wt/no-reflog/target/artifact"
check "--apply: main checkout keeps target/artifact" test -f "$repo/target/artifact"
check "--apply: merged's target/ is removed" removed "$wt/merged/target"

if ((failures)); then
  echo "FAIL: $failures check(s) failed" >&2
  exit 1
fi
echo "PASS"
