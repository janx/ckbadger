# Bulk Sync Hotpath Refactor Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Remove the known parser-side repeated work first, then run controller experiments on a cleaner baseline, then optimize `T1_cells` steady-state cost without mixing the three problems together.

**Architecture:** Treat the bulk-sync bottlenecks as three separate phases. Phase 1 rewrites mNFT/.bit precompute around `TxData`/`ParsedCell` reuse, a single output scan, and one witness parse per tx while preserving canonical inline writes. Phase 2 adds missing pressure signals and updates the adaptive controller only after parser refactor numbers are available. Phase 3 instruments `T1_cells` into measurable subphases and applies a bounded optimization only to the measured dominant slice.

**Tech Stack:** Rust, Tokio, Rayon, tracing, RocksDB, inline unit tests in `ckbadger-indexer` and `ckbadger-store`, bulk-sync perf artifacts under `workdir/perf/bulk-sync`

---

### Task 1: Add reusable .bit witness parse artifacts

**Files:**

- Modify: `crates/indexer/src/parser/dotbit.rs`
- Modify: `crates/indexer/src/sync/types.rs`
- Test: `crates/indexer/src/parser/dotbit.rs`

**Step 1: Write the failing tests**

Add focused unit tests near `crates/indexer/src/parser/dotbit.rs` for a new witness helper that parses a tx's DAS witnesses once and exposes both account data and action string.

```rust
#[test]
fn test_parse_dotbit_witness_bundle_extracts_account_data_and_action_once() {
    let tx = create_dotbit_tx_with_action_and_account("alice.bit", "transfer_account");

    let bundle = parse_dotbit_witness_bundle(&tx.witnesses).expect("bundle");

    assert_eq!(bundle.action.as_deref(), Some("transfer_account"));
    assert_eq!(bundle.accounts.len(), 1);
}

#[test]
fn test_parse_dotbit_witness_bundle_handles_non_das_witnesses() {
    let bundle = parse_dotbit_witness_bundle(&Vec::<String>::new()).expect("bundle");
    assert!(bundle.action.is_none());
    assert!(bundle.accounts.is_empty());
}
```

**Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p ckbadger-indexer test_parse_dotbit_witness_bundle_extracts_account_data_and_action_once -- --nocapture
cargo test -p ckbadger-indexer test_parse_dotbit_witness_bundle_handles_non_das_witnesses -- --nocapture
```

Expected: FAIL because the bundle helper does not exist yet.

**Step 3: Write minimal implementation**

- Add a small parser-local witness bundle type in `crates/indexer/src/parser/dotbit.rs`.
- Refactor current witness parsing so account data and DAS action are extracted from the same decoded witness bytes.
- Add a compact bridge type in `crates/indexer/src/sync/types.rs` only if parser/writer handoff needs it.

**Step 4: Run tests to verify they pass**

Run:

```bash
cargo test -p ckbadger-indexer test_parse_dotbit_witness_bundle_extracts_account_data_and_action_once -- --nocapture
cargo test -p ckbadger-indexer test_parse_dotbit_witness_bundle_handles_non_das_witnesses -- --nocapture
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/indexer/src/parser/dotbit.rs crates/indexer/src/sync/types.rs docs/plans/2026-03-07-bulk-sync-hotpath-refactor.md
git commit -m "refactor: unify dotbit witness parsing"
```

### Task 2: Add a single-pass NFT/.bit output scanner over `TxData`

**Files:**

- Modify: `crates/indexer/src/parser/mnft.rs`
- Modify: `crates/indexer/src/parser/dotbit.rs`
- Modify: `crates/indexer/src/sync/types.rs`
- Modify: `crates/indexer/src/sync/pipeline.rs`
- Test: `crates/indexer/src/sync/pipeline.rs`

**Step 1: Write the failing test**

Add a parser-stage unit test in `crates/indexer/src/sync/pipeline.rs` for a new helper that scans one tx exactly once and produces the same pre-parsed NFT outputs expected by the current multi-pass path.

```rust
#[test]
fn test_scan_preparsed_nft_tx_single_pass_collects_mnft_and_dotbit_outputs() {
    let tx = test_mixed_nft_tx();
    let tx_data = test_mixed_nft_tx_data(&tx);
    let witness_bundle = parse_dotbit_witness_bundle(&tx.witnesses).unwrap();

    let scanned = scan_preparsed_nft_tx(&tx, &tx_data, 0, &witness_bundle).unwrap();

    assert_eq!(scanned.mnft_tokens.len(), 1);
    assert_eq!(scanned.dotbit_accounts.len(), 1);
}
```

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p ckbadger-indexer test_scan_preparsed_nft_tx_single_pass_collects_mnft_and_dotbit_outputs -- --nocapture
```

Expected: FAIL because the single-pass helper does not exist yet.

**Step 3: Write minimal implementation**

- Introduce a parser helper in `crates/indexer/src/sync/pipeline.rs` (or a small parser module if needed) that walks one tx's outputs once.
- Reuse `tx_data.cells` and `tx_data.outputs_data` instead of re-decoding raw RPC output hex for data that already exists in `ParsedCell`.
- Reuse the Task 1 witness bundle instead of calling `.bit` witness parsers again.
- Preserve all existing `PreParsedNftData` semantics.

**Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p ckbadger-indexer test_scan_preparsed_nft_tx_single_pass_collects_mnft_and_dotbit_outputs -- --nocapture
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/indexer/src/parser/mnft.rs crates/indexer/src/parser/dotbit.rs crates/indexer/src/sync/types.rs crates/indexer/src/sync/pipeline.rs
git commit -m "refactor: add single-pass nft tx scanner"
```

### Task 3: Wire parser phase 1 to reuse `ParsedCell` instead of raw re-parse

**Files:**

- Modify: `crates/indexer/src/sync/pipeline.rs`
- Test: `crates/indexer/src/sync/pipeline.rs`

**Step 1: Write the failing test**

Add a focused regression test proving the full parser precompute bridge still produces the same `PreParsedNftData` for a simple mixed batch after the single-pass refactor.

```rust
#[test]
fn test_run_nft_precompute_single_pass_preserves_preparsed_bridge_shape() {
    let input = test_parser_precompute_input_with_mnft_and_dotbit();

    let output = run_nft_precompute_for_test(input).unwrap();

    assert_eq!(output.mnft_tokens.len(), 1);
    assert_eq!(output.dotbit_accounts.len(), 1);
    assert_eq!(output.consumed_dotbit.len(), 1);
}
```

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p ckbadger-indexer test_run_nft_precompute_single_pass_preserves_preparsed_bridge_shape -- --nocapture
```

Expected: FAIL because the extracted/wired helper does not exist yet.

**Step 3: Write minimal implementation**

- Replace the current phase-1 multi-pass loops in `crates/indexer/src/sync/pipeline.rs` with the single-pass helper from Task 2.
- Keep phase-2 consumed `.bit` identification logic intact, but feed it the already parsed tx-level artifacts.
- Remove duplicate `.bit` action parsing and duplicate raw output scans.

**Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p ckbadger-indexer test_run_nft_precompute_single_pass_preserves_preparsed_bridge_shape -- --nocapture
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/indexer/src/sync/pipeline.rs
git commit -m "refactor: reuse parsed cells in nft precompute"
```

### Task 4: Verify parser refactor against the current hotspot windows

**Files:**

- Modify: `crates/indexer/src/sync/pipeline.rs`
- Test: `crates/indexer/src/parser/dotbit.rs`
- Test: `crates/indexer/src/sync/pipeline.rs`

**Step 1: Run targeted parser tests**

Run:

```bash
cargo test -p ckbadger-indexer test_parse_dotbit_witness_bundle_extracts_account_data_and_action_once -- --nocapture
cargo test -p ckbadger-indexer test_scan_preparsed_nft_tx_single_pass_collects_mnft_and_dotbit_outputs -- --nocapture
cargo test -p ckbadger-indexer test_run_nft_precompute_single_pass_preserves_preparsed_bridge_shape -- --nocapture
```

Expected: PASS

**Step 2: Run broader crate verification**

Run:

```bash
cargo test -p ckbadger-indexer --lib -- --nocapture
```

Expected: PASS

**Step 3: Run the fresh-db bulk-sync performance verification**

Run:

```bash
ckbadger purge
ckbadger run
```

Expected:

- Fresh-db bulk sync completes end-to-end on the canonical path.
- A new completed artifact appears under `workdir/perf/bulk-sync/latest/`.
- Compared to the current baseline, the `14.0M` NFT-hot parser window should show materially lower `nft_precompute_ms` and lower `pipeline_parse_ms`, while writer metrics are allowed to stay roughly flat.

**Step 4: Commit**

```bash
git add crates/indexer/src/parser/dotbit.rs crates/indexer/src/parser/mnft.rs crates/indexer/src/sync/pipeline.rs
git commit -m "test: verify parser hotspot refactor"
```

### Task 5: Add the missing controller signals without changing policy yet

**Files:**

- Modify: `crates/ckbadger-store/src/store.rs`
- Modify: `crates/indexer/src/sync/diagnostics.rs`
- Modify: `crates/indexer/src/sync/pipeline.rs`
- Modify: `crates/indexer/src/sync/adaptive.rs`
- Test: `crates/indexer/src/sync/diagnostics.rs`
- Test: `crates/indexer/src/sync/adaptive.rs`

**Step 1: Write the failing tests**

Add tests that prove the diagnostics layer can represent:

- `l0_total` separately from `l0_max`
- parser queue fill separately from writer queue fill
- raw pending txs independently from percentage calculations

```rust
#[test]
fn test_queue_fill_snapshot_keeps_parser_and_writer_pressure_separate() {
    let snapshot = build_queue_pressure_snapshot_for_test(320_000, 1_280_000, 3, 8);

    assert_eq!(snapshot.parse_queue_fill_pct, Some(25.0));
    assert_eq!(snapshot.writer_queue_fill_pct, Some(37.5));
}

#[test]
fn test_compaction_pressure_snapshot_reports_l0_total_and_l0_max() {
    let snapshot = compaction_pressure_snapshot_for_test(82, 3, 0, 0);

    assert_eq!(snapshot.l0_files_total, 82);
    assert_eq!(snapshot.l0_files_max, 3);
}
```

**Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p ckbadger-indexer test_queue_fill_snapshot_keeps_parser_and_writer_pressure_separate -- --nocapture
cargo test -p ckbadger-indexer test_compaction_pressure_snapshot_reports_l0_total_and_l0_max -- --nocapture
```

Expected: FAIL because the new snapshot fields/helpers do not exist yet.

**Step 3: Write minimal implementation**

- Extend the store-side compaction snapshot to expose `l0_files_total` in addition to `l0_files_max`.
- Stop mirroring parser queue fill into writer queue fill.
- Record parser pending txs, parser capacity, writer queue depth, and writer capacity as separate fields in diagnostics/perf samples.
- Keep `update_after_write()` behavior unchanged in this task; this is observability only.

**Step 4: Run tests to verify they pass**

Run:

```bash
cargo test -p ckbadger-indexer test_queue_fill_snapshot_keeps_parser_and_writer_pressure_separate -- --nocapture
cargo test -p ckbadger-indexer test_compaction_pressure_snapshot_reports_l0_total_and_l0_max -- --nocapture
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/ckbadger-store/src/store.rs crates/indexer/src/sync/diagnostics.rs crates/indexer/src/sync/pipeline.rs crates/indexer/src/sync/adaptive.rs
git commit -m "feat: expose controller pressure signals"
```

### Task 6: Run the adaptive/controller experiment on the post-parser baseline

**Files:**

- Modify: `crates/indexer/src/sync/adaptive.rs`
- Modify: `crates/indexer/src/sync/pipeline.rs`
- Test: `crates/indexer/src/sync/adaptive.rs`

**Step 1: Write the failing tests**

Add tests that codify the intended post-parser controller behavior:

- `l0_total` can trigger backoff even when `l0_max` is small.
- writer queue pressure is no longer inferred from parser fill.
- bulk-floor moderate backoff does not silently collapse into a no-op when new severe evidence is present.

```rust
#[test]
fn test_update_after_write_uses_l0_total_pressure() {
    let controller = AdaptiveBatchController::new(8);
    let adjustment = controller.update_after_write(test_input_with_l0_total_pressure()).unwrap();

    assert!(adjustment.new_target_batch_txs < adjustment.previous_target_batch_txs);
}

#[test]
fn test_update_after_write_treats_writer_queue_pressure_independently() {
    let controller = AdaptiveBatchController::new(8);
    let adjustment = controller
        .update_after_write(test_input_with_writer_pressure_only())
        .unwrap();

    assert!(adjustment.new_inflight_limit < adjustment.previous_inflight_limit || adjustment.new_target_batch_txs < adjustment.previous_target_batch_txs);
}
```

**Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p ckbadger-indexer test_update_after_write_uses_l0_total_pressure -- --nocapture
cargo test -p ckbadger-indexer test_update_after_write_treats_writer_queue_pressure_independently -- --nocapture
```

Expected: FAIL because controller inputs/logic do not use those signals yet.

**Step 3: Write minimal implementation**

- Extend `AdaptiveBatchInput` with the new pressure fields.
- Update `update_after_write()` so `l0_total` and real writer pressure participate in backoff decisions.
- Keep the parser refactor out of this task; use only the new post-parser baseline to judge whether the policy change helps.

**Step 4: Run tests to verify they pass**

Run:

```bash
cargo test -p ckbadger-indexer test_update_after_write_uses_l0_total_pressure -- --nocapture
cargo test -p ckbadger-indexer test_update_after_write_treats_writer_queue_pressure_independently -- --nocapture
```

Expected: PASS

**Step 5: Run experiment verification**

Run:

```bash
cargo test -p ckbadger-indexer --lib -- --nocapture
ckbadger purge
ckbadger run
```

Expected:

- Bulk-sync parser metrics stay improved relative to the post-Task-4 baseline.
- Backoff clusters should appear earlier in controller logs when `l0_total` is high or writer backlog is real.
- Compare the new `workdir/perf/bulk-sync/latest/report.md` against the Task-4 baseline rather than against the pre-parser baseline.

**Step 6: Commit**

```bash
git add crates/indexer/src/sync/adaptive.rs crates/indexer/src/sync/pipeline.rs
git commit -m "feat: update adaptive backoff pressure signals"
```

### Task 7: Split `T1_cells` into measurable subphases

**Files:**

- Modify: `crates/indexer/src/sync/batch.rs`
- Modify: `crates/indexer/src/db/writer/cells.rs`
- Test: `crates/indexer/src/sync/batch.rs`

**Step 1: Write the failing test**

Add a focused unit test around a new `T1Breakdown` helper so the write-path metrics can distinguish:

- live/canonical payload writes
- consumed payload writes
- index puts
- index deletes

```rust
#[test]
fn test_t1_breakdown_total_matches_component_sum() {
    let breakdown = T1Breakdown {
        payload_insert_ms: 10.0,
        payload_consume_ms: 20.0,
        index_put_ms: 30.0,
        index_delete_ms: 40.0,
    };

    assert_eq!(breakdown.total_ms(), 100.0);
}
```

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p ckbadger-indexer test_t1_breakdown_total_matches_component_sum -- --nocapture
```

Expected: FAIL because the helper/metrics do not exist yet.

**Step 3: Write minimal implementation**

- Add per-subphase timing inside the `T1_cells` worker in `crates/indexer/src/sync/batch.rs`.
- Log the subphase timings in the batch write breakdown.
- Keep behavior identical; this task is measurement only.

**Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p ckbadger-indexer test_t1_breakdown_total_matches_component_sum -- --nocapture
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/indexer/src/sync/batch.rs crates/indexer/src/db/writer/cells.rs
git commit -m "feat: split t1 cell write metrics"
```

### Task 8: Optimize the measured dominant `T1_cells` slice

**Files:**

- Modify: `crates/indexer/src/db/writer/cells.rs`
- Modify: `crates/ckbadger-store/src/batch.rs`
- Modify: `crates/indexer/src/sync/batch.rs`
- Test: `crates/indexer/src/db/writer/cells.rs`
- Test: `crates/ckbadger-store/src/batch.rs`

**Step 1: Write the failing test**

Add regression tests for raw-key helpers that let `T1_cells` reuse pre-encoded outpoint keys instead of re-encoding them for payload/live/consumed operations.

```rust
#[test]
fn test_put_cell_raw_key_matches_put_cell_encoding() {
    let mut batch = test_store_batch();
    let info = test_live_cell_info();
    let raw_key = keys::encode_outpoint(&[0x11; 32], 3);

    batch.put_cell_raw_key(&raw_key, &info);

    assert_eq!(batch.debug_op_count_for_cf("cells"), 1);
    assert_eq!(batch.debug_op_count_for_cf("live_cells"), 1);
}
```

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p ckbadger-store test_put_cell_raw_key_matches_put_cell_encoding -- --nocapture
```

Expected: FAIL because raw-key payload helpers do not exist yet.

**Step 3: Write minimal implementation**

- Add raw-key helpers in `crates/ckbadger-store/src/batch.rs` for cell/live/consumed payload operations.
- Precompute and reuse encoded outpoint keys inside `T1_cells` instead of encoding them multiple times.
- Limit this task to the measured dominant `T1` slice from Task 7; do not add broader unrelated refactors.

**Step 4: Run tests to verify they pass**

Run:

```bash
cargo test -p ckbadger-store test_put_cell_raw_key_matches_put_cell_encoding -- --nocapture
cargo test -p ckbadger-indexer --lib -- --nocapture
```

Expected: PASS

**Step 5: Run steady-state verification**

Run:

```bash
ckbadger purge
ckbadger run
```

Expected:

- Parser improvements from Tasks 1-4 remain intact.
- Controller behavior from Tasks 5-6 remains intact.
- `T1_cells` subphase timings show the dominant slice reduced versus the Task-7 baseline.

**Step 6: Commit**

```bash
git add crates/ckbadger-store/src/batch.rs crates/indexer/src/db/writer/cells.rs crates/indexer/src/sync/batch.rs
git commit -m "refactor: reduce t1 cell payload encoding overhead"
```

### Task 9: Final end-to-end verification and perf summary

**Files:**

- Modify: `docs/INDEXER_PIPELINE.md`
- Modify: `docs/plans/2026-03-07-bulk-sync-hotpath-refactor.md`

**Step 1: Run final targeted tests**

Run:

```bash
cargo test -p ckbadger-indexer --lib -- --nocapture
cargo test -p ckbadger-store --lib -- --nocapture
```

Expected: PASS

**Step 2: Run final fresh-db bulk-sync verification**

Run:

```bash
ckbadger purge
ckbadger run
```

Expected:

- Bulk sync finishes on the canonical path.
- `workdir/perf/bulk-sync/latest/report.md` reflects the final post-parser, post-controller, post-T1 state.
- Before/after notes clearly separate:
  - parser hotspot delta
  - controller/backoff delta
  - `T1_cells` steady-state delta

**Step 3: Update the architecture docs**

- Update `docs/INDEXER_PIPELINE.md` only for behavior that actually changed.
- Keep the documentation aligned with the final parser/controller/T1 structure.

**Step 4: Commit**

```bash
git add docs/INDEXER_PIPELINE.md docs/plans/2026-03-07-bulk-sync-hotpath-refactor.md
git commit -m "docs: record bulk sync hotpath refactor verification"
```

Plan complete and saved to `docs/plans/2026-03-07-bulk-sync-hotpath-refactor.md`.

Two execution options:

1. Subagent-Driven (this session) - I dispatch fresh subagent per task, review between tasks, fast iteration
2. Parallel Session (separate) - Open new session with executing-plans, batch execution with checkpoints

If no preference is given, use option 1 in this session.
