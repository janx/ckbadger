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

Commit: 0b399d39

## Task 4.2 — read-only entity statistics export under one read pin

RED (`cargo test -p ckbadger-api --test api_verify`): first a compile error
(`set_bulk_build_session_marker` takes `Option<&_>`), then
`test result: FAILED. 1 passed; 10 failed` — every case panicked with
"response body must be JSON (EOF while parsing a value at line 1 column 0)"
because the route did not exist (404, empty body).

GREEN: `cargo test -p ckbadger-api --test api_verify` → 11 passed; 0 failed.
`cargo clippy -p ckbadger-api --all-targets` clean after deriving `Debug` on
`ResolvedEntity` and making `routes::verify` a `pub` module (the
`HourlyRetentionReport::State` variant is constructed only by Phase 2, so a
`pub(crate)` module would have needed a dead-code allow).

Phase-2 placeholders pinned by `phase2_state_fields_report_absence_of_evidence`:
`state.entityStatsUndoContract == null` and `state.hourlyRetention == "unknown"`,
both carrying `// TODO(phase2 merge)` in `routes/verify.rs`.

Pre-existing, unrelated failures in this worktree: 5 `ckbadger-api --lib` tests
(`entry::tests::*`, `frontend_formats::tests::origin_uses_...`) fail with
"Missing embedded agent renderer: run pnpm --dir frontend build" — the worktree
has no built `frontend/dist`. Not touched by this work.

Commit: 8e4d8ad2

## Task 4.3 — indexer RPC methods + history source qualification

`wiremock` was already a dev-dependency of `crates/indexer` (workspace 0.6), so
no Cargo.toml change was needed; recorded in the plan as an 实施记录.

RED (`cargo test -p ckbadger-indexer --lib verify::source`): compile errors —
`no IndexerSearchKey in rpc::client`, `cannot find type SourceAnchor`,
`cannot find struct PaginationBudget`, `cannot find function qualify_source /
collect_transactions / SourceDeclaration::load`.

GREEN:

- `cargo test -p ckbadger-indexer --lib verify::source` → 12 passed.
- `cargo test -p ckbadger-indexer --lib rpc::client` → 9 passed (4 new).
- `cargo clippy -p ckbadger-indexer --all-targets` clean.

Note: the new `parse_indexer_hex_u32` is deliberately *not* the existing
`ckbadger_common::parse_hex_u32`, which panics on bad input — a panic at the
RPC boundary aborts the release binary.

Commit: ac7179e6

## Task 4.4 — entity_capacity_history_matches_chain (token family)

RED (`cargo test -p ckbadger-indexer --test verify_entity_statistics`):
compile errors — `no EntitySelector in verify::checks`, `could not find
entity_history in verify`, `CheckContext has no field entities /
verify_source_path / evidence_dir`.

Two intermediate reds worth recording:

1. `the_script_hash_matches_ckbs_own` failed against an invented golden
   constant. Replaced with a cross-check against an independently written
   molecule `Script` encoding + blake2b, plus a hash_type-sensitivity test and
   an unknown-hash_type rejection test — no fabricated golden.
2. All 8 integration cases failed with "Cannot drop a runtime in a context
   where blocking is not allowed". Two distinct causes, both real:
   - **production defect**: the check built a tokio `Runtime` while running on
     a blocking thread that still carries the caller's tokio context (the CLI
     calls `verify::run` from `spawn_blocking`). Fixed by
     `run_on_dedicated_runtime`, which drives the whole chain phase on a plain
     OS thread, so the runtime's lifetime never touches the caller's context.
   - **test defect**: `reqwest::blocking::Client::new()` was called from the
     async test body (it spins up and drops a temporary runtime). The
     `CheckContext` is now built inside the blocking worker.

GREEN:

- `cargo test -p ckbadger-indexer --test verify_entity_statistics` → 9 passed.
- `cargo test -p ckbadger-indexer --lib verify` → all green inside
  1531 lib tests, 0 failed.
- `cargo test -p ckbadger-indexer` (all targets) → 0 failed.
- `cargo test -p ckbadger` → 127 + 4, 0 failed.
- `cargo test -p ckbadger-api --test api_verify` → 11 passed.
- `cargo fmt --all -- --check` clean; `cargo clippy -p ckbadger-indexer -p
  ckbadger-api -p ckbadger --all-targets` clean.

Check count is now 58 (31 api + 26 explorer + 1 new), matching the plan.

## Docs (Phase 7 verify bullets only)

- `docs/TESTING.md`: 57 → 58, Sampling S1-S25, new "Check Statuses and Exit
  Codes" and "Chain-derived Entity Verification (S25)" sections, `--entity`
  flag, `verify-source.toml` example, file-location rows for the new modules.
- `docs/API.md`: 18 → 19 modules, 127 → 128 endpoints, new `verify` module
  section with the full request/response contract and error cases.
- `CLAUDE.md`'s "57 checks" line is deliberately **not** touched: this task's
  scope names only `docs/TESTING.md` and `docs/API.md`. It needs the same 57 →
  58 update when Phase 7 is finished.

