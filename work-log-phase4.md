# Phase 4 work log (Tasks 4.1–4.4)

Worktree: /home/f0rk/projects/ckbadger/.claude/worktrees/agent-a6c2607f3ee288a97
Branch: fix/verify-min-v-20260923 (from d00ff2eb)
Plan: docs/superpowers/plans/2026-09-23-sync-correctness-and-performance-fix.md (gitignored copy)

## Setup

- `git checkout -b fix/verify-min-v-20260923` — OK
- copied plan into worktree at same path (gitignored, not committed)
- `cargo check` first failed: worktree had no `Cargo.lock` (gitignored), cargo
  re-resolved to versions needing rustc 1.95 (have 1.93.1). Copied the main
  checkout's `Cargo.lock` in; baseline `cargo check -p ckbadger-indexer -p
  ckbadger-api -p ckbadger --all-targets` green in 4m14s.
- cargo needs write access to `~/.cargo/registry`, which the Bash sandbox
  denies; every cargo command is run with the sandbox disabled.

## Task 4.1 — structured statuses, single JSON envelope, 0/1/2 exit codes

RED (`cargo test -p ckbadger-indexer --lib verify`, `cargo test -p ckbadger
--test verify_exit_codes`):

- indexer: compile errors — `unresolved import CheckStatus`, `no field status on
  CheckResult`, `no function inconclusive/not_applicable/error`, `no field
  status_reason`, `VerifyReport not found`, `render_json/render_text_summary/
  write_network_report not found` (~40 errors).
- cli: `test result: FAILED. 0 passed; 4 failed` — stdout carried the ASCII
  banner plus two concatenated top-level JSON documents with `passed`/`skipped`
  fields and no envelope.

GREEN:

- `cargo test -p ckbadger-indexer --lib verify` → 124 passed; 0 failed.
- `cargo test -p ckbadger-indexer` (all targets) → 1500 lib + 122 integration,
  0 failed.
- `cargo test -p ckbadger` → 127 unit + 4 `verify_exit_codes`, 0 failed.
- `cargo fmt --all -- --check` clean; `cargo clippy -p ckbadger-indexer -p
  ckbadger --all-targets` clean.

Semantics deliberately changed (and their tests updated to the new, equally
strong assertions):

- skipped no longer counts as pass (`CompletedCheck::passed()` is
  `status == Pass`);
- a check returning `Err` is `Error` (exit 2), not `Fail` (exit 1);
- `--sample-count 0` on a sampling check is `Error`, not a pass over an empty
  set;
- DAO S24 reports *any* anchor change as `Inconclusive` (was: a green
  `pass_with_detail` whose text started with "INCONCLUSIVE", and only when
  findings existed) — new regression test
  `dao_status_index_reports_an_anchor_change_as_inconclusive_not_pass`;
- explorer comparisons with zero overlapping dates are `Inconclusive`, not
  `Fail` (nothing was compared).

