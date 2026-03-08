# Bulk Sync Wall-Clock Optimization Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Reduce fresh-db bulk sync wall-clock from ~5,445s to ~3,100-3,600s via four independent optimizations: controller policy rollback, commit consolidation, T1 raw-key reuse, and L0 threshold tuning.

**Architecture:** The four optimizations target independent bottlenecks. Task 1 fixes a controller regression (batch count). Tasks 2-3 reduce RocksDB commit overhead (structural). Task 4 reduces key encoding cost. Task 5 raises compaction thresholds. Each task produces a measurable delta; order is deliberate (biggest win first, dependencies second).

**Tech Stack:** Rust, RocksDB (via rust-rocksdb), Tokio, std::thread::scope for parallel writes, inline unit tests in ckbadger-indexer and ckbadger-store, bulk-sync perf artifacts under `temp/perf/bulk-sync/`

**Design doc:** `docs/plans/2026-03-08-bulk-sync-wall-clock-optimization-design.md`

---

### Task 1: Revert controller policy to pre-regression behavior

**Files:**

- Modify: `crates/indexer/src/sync/adaptive.rs`

**Step 1: Write the failing tests**

Add tests proving the reverted policy does NOT back off on `l0_files_total` alone and does NOT gate moderate/severe pressure behind `far_bulk_cost_backoff_allowed`.

```rust
// In mod tests at bottom of adaptive.rs

#[test]
fn test_reverted_policy_l0_total_does_not_trigger_backoff() {
    // l0_files_total=200 should not cause any backoff because the
    // reverted policy only uses l0_files_max, not l0_files_total.
    let controller = AdaptiveBatchController::new(8);
    controller
        .target_batch_txs
        .store(ADAPTIVE_BATCH_MAX_TXS, Ordering::Relaxed);
    controller
        .inflight_limit
        .store(ADAPTIVE_BATCH_BULK_DISTANCE_MIN_INFLIGHT, Ordering::Relaxed);
    controller.min_target_batch_txs.store(
        ADAPTIVE_BATCH_BULK_DISTANCE_MIN_TARGET_TXS,
        Ordering::Relaxed,
    );

    let adjustment = controller.update_after_write(AdaptiveBatchInput {
        write_ms: 800.0,
        commit_ms: 400.0,
        batch_tx_count: 10_000,
        blocks_remaining: ADAPTIVE_BATCH_NEAR_TIP_THRESHOLD_BLOCKS + 10_000,
        parse_queue_fill_pct: Some(10.0),
        writer_queue_fill_pct: Some(10.0),
        memory_ratio_pct: Some(10.0),
        l0_files_total: Some(200),
        l0_files_max: Some(5),
        compaction_pending_bytes: None,
        immutable_memtables: None,
        severe_pending_threshold: 8 * 1024 * 1024 * 1024,
        moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
        severe_imm_threshold: 60,
        moderate_imm_threshold: 30,
    });

    assert!(
        adjustment.is_none(),
        "l0_files_total must not trigger backoff — only l0_files_max matters"
    );
}

#[test]
fn test_reverted_policy_rocksdb_moderate_pressure_not_gated_by_write_cost() {
    // rocksdb_moderate_pressure (l0_max=25) should trigger backoff
    // regardless of write cost, i.e. no far_bulk_cost_backoff_allowed gate.
    let controller = AdaptiveBatchController::new(8);
    controller
        .target_batch_txs
        .store(ADAPTIVE_BATCH_MAX_TXS, Ordering::Relaxed);
    controller
        .inflight_limit
        .store(ADAPTIVE_BATCH_BULK_DISTANCE_MIN_INFLIGHT, Ordering::Relaxed);
    controller.min_target_batch_txs.store(
        ADAPTIVE_BATCH_BULK_DISTANCE_MIN_TARGET_TXS,
        Ordering::Relaxed,
    );

    let adjustment = controller.update_after_write(AdaptiveBatchInput {
        write_ms: 200.0,  // very healthy
        commit_ms: 50.0,  // very healthy
        batch_tx_count: 10_000,
        blocks_remaining: ADAPTIVE_BATCH_NEAR_TIP_THRESHOLD_BLOCKS + 10_000,
        parse_queue_fill_pct: Some(10.0),
        writer_queue_fill_pct: Some(10.0),
        memory_ratio_pct: Some(10.0),
        l0_files_total: None,
        l0_files_max: Some(25),
        compaction_pending_bytes: None,
        immutable_memtables: None,
        severe_pending_threshold: 8 * 1024 * 1024 * 1024,
        moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
        severe_imm_threshold: 60,
        moderate_imm_threshold: 30,
    });

    assert!(
        adjustment.is_some(),
        "l0_files_max=25 should trigger moderate backoff even with healthy write cost"
    );
    assert_eq!(adjustment.unwrap().reason, "moderate_backoff");
}
```

**Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p ckbadger-indexer test_reverted_policy_l0_total_does_not_trigger_backoff -- --nocapture
cargo test -p ckbadger-indexer test_reverted_policy_rocksdb_moderate_pressure_not_gated_by_write_cost -- --nocapture
```

Expected: First test FAILs because `l0_files_total=200 >= ADAPTIVE_BATCH_SEVERE_L0_TOTAL_FILES(80)` currently triggers backoff. Second test FAILs because `far_bulk_cost_backoff_allowed` is false (healthy cost + far bulk), so `rocksdb_moderate_pressure` is gated.

**Step 3: Write minimal implementation**

Revert the policy in `update_after_write()`:

1. Remove `l0_files_total` from `rocksdb_severe_pressure` and `rocksdb_moderate_pressure`:

```rust
// BEFORE (current):
let rocksdb_severe_pressure = input
    .l0_files_total
    .is_some_and(|l0| l0 >= ADAPTIVE_BATCH_SEVERE_L0_TOTAL_FILES)
    || input.l0_files_max.is_some_and(|l0| l0 >= 40)
    || ...;
let rocksdb_moderate_pressure = input
    .l0_files_total
    .is_some_and(|l0| l0 >= ADAPTIVE_BATCH_MODERATE_L0_TOTAL_FILES)
    || input.l0_files_max.is_some_and(|l0| l0 >= 20)
    || ...;

// AFTER (reverted):
let rocksdb_severe_pressure = input.l0_files_max.is_some_and(|l0| l0 >= 40)
    || input
        .compaction_pending_bytes
        .is_some_and(|b| b >= input.severe_pending_threshold)
    || input
        .immutable_memtables
        .is_some_and(|imm| imm >= input.severe_imm_threshold);
let rocksdb_moderate_pressure = input.l0_files_max.is_some_and(|l0| l0 >= 20)
    || input
        .compaction_pending_bytes
        .is_some_and(|b| b >= input.moderate_pending_threshold)
    || input
        .immutable_memtables
        .is_some_and(|imm| imm >= input.moderate_imm_threshold);
```

2. Remove `healthy_absolute_write_cost` and `far_bulk_cost_backoff_allowed`:

```rust
// DELETE these lines:
let healthy_absolute_write_cost = input.write_ms < ADAPTIVE_BATCH_WRITE_LO_MS
    && input.commit_ms < ADAPTIVE_BATCH_HEALTHY_BONUS_COMMIT_MS
    && write_us_per_tx.is_some_and(|us| us < ADAPTIVE_BATCH_WRITE_HEALTHY_US_PER_TX);
let far_bulk_cost_backoff_allowed = near_tip || !healthy_absolute_write_cost;
```

3. Remove `far_bulk_cost_backoff_allowed` gates from `severe_pressure_signal` and `moderate_pressure`:

```rust
// BEFORE:
let severe_pressure_signal = ...
    || (rocksdb_severe_pressure && far_bulk_cost_backoff_allowed);
// AFTER:
let severe_pressure_signal = ...
    || rocksdb_severe_pressure;

// BEFORE:
let moderate_pressure = ...
    || (queue_pressure && throughput_drop_under_load && far_bulk_cost_backoff_allowed)
    ...
    || (rocksdb_moderate_pressure && far_bulk_cost_backoff_allowed);
// AFTER:
let moderate_pressure = ...
    || (queue_pressure && throughput_drop_under_load)
    ...
    || rocksdb_moderate_pressure;
```

4. Revert `severe_floor_relaxation` to simple `severe_pressure`:

```rust
// BEFORE:
let severe_floor_relaxation = !near_tip
    && (severe_pressure || (severe_pressure_signal && reason == Some("moderate_backoff")));
// AFTER:
let severe_floor_relaxation = !near_tip && severe_pressure;
```

5. Revert `min_target_batch_txs` clamp:

```rust
// BEFORE (line 311):
.clamp(ADAPTIVE_BATCH_HARD_MIN_TXS, ADAPTIVE_BATCH_MAX_TXS);
// AFTER:
.clamp(ADAPTIVE_BATCH_HARD_MIN_TXS, ADAPTIVE_BATCH_BASE_MIN_TXS);
```

6. Keep `l0_files_total` field in `AdaptiveBatchInput` — it's still useful for perf logging. Keep `ADAPTIVE_BATCH_MODERATE_L0_TOTAL_FILES` and `ADAPTIVE_BATCH_SEVERE_L0_TOTAL_FILES` constants — they may be useful later. Just remove them from decision paths.

7. Fix any existing tests that relied on the gated behavior — the three tests added by `10d8627`/`e9aa560`:
   - `test_update_after_write_l0_total_only_does_not_backoff_in_far_bulk` → DELETE (tested the gating behavior we're removing)
   - `test_update_after_write_writer_only_pressure_does_not_shard_healthy_far_bulk_batches` → DELETE (tested `far_bulk_cost_backoff_allowed`)
   - `test_update_after_write_severe_hint_in_far_bulk_does_not_noop_at_bulk_floor` → UPDATE to expect `severe_pressure_backoff` instead of `moderate_backoff` since the ungated `rocksdb_severe_pressure` now triggers directly

**Step 4: Run tests to verify they pass**

Run:

```bash
cargo test -p ckbadger-indexer sync::adaptive::tests:: -- --nocapture
```

Expected: All tests PASS.

**Step 5: Commit**

```bash
git add crates/indexer/src/sync/adaptive.rs
git commit -m "refactor: revert adaptive controller policy to pre-regression behavior

Remove l0_files_total from pressure decisions and far_bulk_cost_backoff_allowed
gating. Keep l0_files_total field for diagnostics. Recovers best-run batch
distribution (3807 batches @ 4934 avg blocks vs 4348 @ 4321)."
```

---

### Task 2: Add StoreBatch merge support

**Files:**

- Modify: `crates/ckbadger-store/src/batch.rs`

**Step 1: Write the failing test**

```rust
// In batch.rs, add a test module if not present, or add to existing #[cfg(test)]

#[cfg(test)]
mod merge_tests {
    use super::*;
    use crate::store::CkbadgerStore;

    fn test_store() -> CkbadgerStore {
        let dir = tempfile::tempdir().unwrap();
        CkbadgerStore::open_for_test(dir.path()).unwrap()
    }

    #[test]
    fn test_merge_domain_batches() {
        let store = test_store();
        let mut a = StoreBatch::new(&store);
        let mut b = StoreBatch::new(&store);

        a.put_tx_hash_map(&[0x11; 32], 1, 0);
        b.put_tx_hash_map(&[0x22; 32], 2, 0);

        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);

        a.merge_from(b);

        assert_eq!(a.len(), 2);
    }
}
```

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p ckbadger-store merge_tests::test_merge_domain_batches -- --nocapture
```

Expected: FAIL — `merge_from` method does not exist.

**Step 3: Write minimal implementation**

Add to `StoreBatch`:

```rust
/// Merge another StoreBatch into this one.
/// Both batches must target the same store.
/// Consumes the other batch.
pub fn merge_from(&mut self, other: StoreBatch<'a>) {
    let (other_wb, other_append_ops, _other_store) = other.into_parts();
    self.batch.append(other_wb);
    self.append_ops.extend(other_append_ops);
}

/// Decompose into raw parts for manual merge.
/// Returns (WriteBatch, append_ops, store_ref).
pub fn into_parts(self) -> (WriteBatch, Vec<AppendBatchOp>, &'a CkbadgerStore) {
    (self.batch, self.append_ops, self.store)
}
```

Note: `WriteBatch::append` is a method on rocksdb's `WriteBatch` that appends all operations from another WriteBatch. Check if `rust-rocksdb` exposes it. If not, we need `into_data()` + `from_data()` or an alternative approach.

If `WriteBatch::append` is not available in the rust-rocksdb version, use `WriteBatch::data()` to extract raw bytes and rebuild. But first check what's available.

Also make `AppendBatchOp` visible within the crate (it's currently private).

**Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p ckbadger-store merge_tests:: -- --nocapture
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/ckbadger-store/src/batch.rs
git commit -m "feat: add StoreBatch::merge_from for commit consolidation"
```

---

### Task 3: Consolidate bulk sync parallel commits to 2 per batch

**Files:**

- Modify: `crates/indexer/src/sync/batch.rs`

**Step 1: Write the failing test**

Add a test proving that the consolidated commit path produces the same commit count as expected. This is an integration-level assertion — the best verification is the full test suite + fresh-db run. But we can add a focused test:

```rust
#[test]
fn test_consolidated_bulk_commit_count() {
    // Verify the function signature and return type exist.
    // The real verification is cargo test + fresh-db run.
    // This test just checks the refactored thread returns uncommitted batches.
    //
    // NOTE: This is a compile-time shape test; the real validation is the
    // existing integration tests in reorg_handling.rs + api_integration.rs
    // continuing to pass with the new commit path.
}
```

Given the complexity, skip a synthetic unit test and instead rely on existing integration tests as the regression check.

**Step 2: Verify existing tests pass before refactor**

Run:

```bash
cargo test -p ckbadger-indexer --lib -- --nocapture
cargo test -p ckbadger-indexer reorg_handling -- --nocapture
```

Expected: PASS (baseline before refactor).

**Step 3: Write minimal implementation**

Refactor the `thread::scope` block in `write_parsed_batch()` (bulk sync path, starting around line 3614):

1. Change each thread's return type from `Result<(f64, f64)>` (write*ms, commit_ms) to `Result<(f64, StoreBatch<'*>, Option<StoreBatch<'\_>>)>` (write_ms, domain_batch, optional_append_batch).

2. Remove `commit_phase_no_wal()` calls inside each thread. Instead, return the uncommitted StoreBatch.

3. After all threads join, merge all domain batches and all append batches:

```rust
// After thread::scope, collect results:
let (t1_ms, t1_domain, _) = h1.join().expect("T1 panicked")?;
let (t2_ms, t2_domain, t2_append) = h2.join().expect("T2 panicked")?;
let (t4_ms, t4_domain, _) = h4.join().expect("T4 panicked")?;
let (t5_ms, t5_domain, _) = h5.join().expect("T5 panicked")?;
let (t6a_ms, t6a_domain, t6a_append) = h6a.join().expect("T6a panicked")?;
let (t6b_ms, t6b_domain, t6b_append) = h6b.join().expect("T6b panicked")?;
let (stats, t7_ms) = h7.join().expect("T7 panicked")?;
let (t_act_ms, t_act_domain, t_act_append) = match h_act {
    Some(h) => { let r = h.join().expect("T_ACT panicked")?; (r.0, Some(r.1), Some(r.2)) }
    None => (0.0, None, None),
};

// Merge domain batches
let mut merged_domain = t1_domain;
merged_domain.merge_from(t2_domain);
merged_domain.merge_from(t4_domain);
merged_domain.merge_from(t5_domain);
merged_domain.merge_from(t6a_domain);
merged_domain.merge_from(t6b_domain);
if let Some(act_domain) = t_act_domain {
    merged_domain.merge_from(act_domain);
}

// Add finalize data (block headers + stats) to merged domain batch
self.writer.insert_blocks_batch(&block_refs, &mut merged_domain)?;
self.write_batch_stats_to_batch(&batch_stats, &mut merged_domain)?;

// Single domain commit
let domain_commit_start = Instant::now();
merged_domain.commit_no_wal()?;
let domain_commit_ms = domain_commit_start.elapsed().as_secs_f64() * 1000.0;

// Merge append batches
let append_store = &self.append_only_store;
let mut merged_append = StoreBatch::new(append_store);
if let Some(a) = t2_append { merged_append.merge_from(a); }
if let Some(a) = t6a_append { merged_append.merge_from(a); }
if let Some(a) = t6b_append { merged_append.merge_from(a); }
if let Some(a) = t_act_append.flatten() { merged_append.merge_from(a); }

// Single append commit
let append_commit_ms = if !merged_append.is_empty() {
    let t = Instant::now();
    merged_append.commit_no_wal()?;
    t.elapsed().as_secs_f64() * 1000.0
} else {
    0.0
};

write_commit_ms = domain_commit_ms + append_commit_ms;
```

4. The finalize section (previously lines ~2887-2902 / ~6150-6190) is now absorbed into the merged domain batch above. Remove the separate `core_batch` and `stats_batch` creation and commit.

5. Leave the live sync path (`else` branch starting around line 4736) unchanged.

**Step 4: Run tests to verify they pass**

Run:

```bash
cargo test -p ckbadger-indexer --lib -- --nocapture
cargo test -p ckbadger-indexer reorg_handling -- --nocapture
cargo test -p ckbadger-api -- --nocapture
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/indexer/src/sync/batch.rs
git commit -m "perf: consolidate bulk sync commits from ~13 to 2 per batch

Threads return uncommitted StoreBatch; main thread merges all domain
batches into one WriteBatch and all append batches into another.
Single domain commit + single append commit per batch.
Live sync path unchanged."
```

---

### Task 4: T1 raw-key reuse for cell operations

**Files:**

- Modify: `crates/ckbadger-store/src/batch.rs`
- Modify: `crates/indexer/src/db/writer/cells.rs`

**Step 1: Write the failing test**

```rust
// In crates/ckbadger-store/src/batch.rs #[cfg(test)]

#[test]
fn test_put_cell_raw_key_produces_same_result() {
    let store = test_store();
    let tx_hash = [0x33u8; 32];
    let output_index: i16 = 2;
    let raw_key = keys::encode_outpoint(&tx_hash, output_index);

    let info = LiveCellInfo {
        capacity: 100_00000000,
        lock_script_hash: vec![0xaa; 32],
        lock_code_hash: vec![0xbb; 32],
        lock_hash_type: 1,
        lock_args: vec![0xcc; 20],
        type_script_hash: Some(vec![0xdd; 32]),
        type_code_hash: Some(vec![0xee; 32]),
        type_hash_type: Some(1),
        type_args: Some(vec![0xff; 20]),
        data_size: 64,
        output_index,
        tx_hash: tx_hash.to_vec(),
        block_number: 42,
    };

    let mut batch = StoreBatch::new(&store);
    batch.put_cell_raw_key(&raw_key, &info);
    batch.commit().unwrap();

    // Verify cell is readable via normal get path
    let cell = store.get_cell(&tx_hash, output_index).unwrap();
    assert!(cell.is_some());
    assert_eq!(cell.unwrap().capacity, 100_00000000);
}
```

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p ckbadger-store test_put_cell_raw_key_produces_same_result -- --nocapture
```

Expected: FAIL — `put_cell_raw_key` does not exist.

**Step 3: Write minimal implementation**

Add raw-key methods to `StoreBatch` in `crates/ckbadger-store/src/batch.rs`:

```rust
/// Insert cell using pre-encoded outpoint key. Avoids re-encoding for
/// callers that need the same key across multiple CF operations.
pub fn put_cell_raw_key(&mut self, raw_key: &[u8], info: &LiveCellInfo) {
    let cf = self.store.cf(CF_CELLS);
    let value = bincode::serialize(info).expect("serialize LiveCellInfo");
    self.put_cf_raw(cf, raw_key, &value);
    let live_cf = self.store.cf(CF_LIVE_CELLS);
    self.put_cf_raw(live_cf, raw_key, &[]);
}

pub fn consume_cell_raw_key(&mut self, raw_key: &[u8], consumed_info: &ConsumedCellInfo) {
    let cf = self.store.cf(CF_CONSUMED_CELLS);
    let value = bincode::serialize(consumed_info).expect("serialize ConsumedCellInfo");
    self.put_cf_raw(cf, raw_key, &value);
    let live_cf = self.store.cf(CF_LIVE_CELLS);
    self.delete_cf_raw(live_cf, raw_key);
}

fn put_cf_raw(&mut self, cf: &ColumnFamily, key: &[u8], value: &[u8]) {
    if self.store.is_append_only_store() {
        let cf_name = self
            .store
            .append_cf_name_for_handle(cf)
            .unwrap_or_else(|e| panic!("resolve append CF: {}", e));
        self.append_ops.push(AppendBatchOp {
            cf_name,
            key: key.to_vec(),
            value: Some(value.to_vec()),
        });
    }
    self.batch.put_cf(cf, key, value);
}

fn delete_cf_raw(&mut self, cf: &ColumnFamily, key: &[u8]) {
    if self.store.is_append_only_store() {
        let cf_name = self
            .store
            .append_cf_name_for_handle(cf)
            .unwrap_or_else(|e| panic!("resolve append CF: {}", e));
        self.append_ops.push(AppendBatchOp {
            cf_name,
            key: key.to_vec(),
            value: None,
        });
    }
    self.batch.delete_cf(cf, key);
}
```

Then update `insert_cells_batch` and `consume_cells_batch_preloaded` in `crates/indexer/src/db/writer/cells.rs` to pre-encode the outpoint key once and use the raw-key methods:

```rust
// In insert_cells_batch, for each cell:
let raw_key = keys::encode_outpoint(&cell.tx_hash, cell.output_index);
batch.put_cell_raw_key(&raw_key, &live_cell_info);

// In consume_cells_batch_preloaded, for each consumption:
let raw_key = keys::encode_outpoint(&input.previous_tx_hash, output_index);
batch.consume_cell_raw_key(&raw_key, &consumed_info);
```

**Step 4: Run tests to verify they pass**

Run:

```bash
cargo test -p ckbadger-store test_put_cell_raw_key -- --nocapture
cargo test -p ckbadger-indexer --lib -- --nocapture
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/ckbadger-store/src/batch.rs crates/indexer/src/db/writer/cells.rs
git commit -m "perf: pre-encode outpoint keys for T1 cell writes

Encode outpoint key once per cell and reuse across CF_CELLS,
CF_LIVE_CELLS, and CF_CONSUMED_CELLS operations."
```

---

### Task 5: Raise L0 thresholds during bulk sync

**Files:**

- Modify: `crates/ckbadger-store/src/store.rs`

**Step 1: Write the failing test**

```rust
// In store.rs tests or a new test

#[test]
fn test_bulk_sync_l0_thresholds_are_raised() {
    // Verify the constants in enter_bulk_sync_mode use the new values.
    // This is a compile-time check via grep/assertion on the set_options call.
    // Real verification: the set_options_cf call uses "96" and "192".
    let dir = tempfile::tempdir().unwrap();
    let store = CkbadgerStore::open_for_test(dir.path()).unwrap();
    store.enter_bulk_sync_mode();
    // If we can read back L0 options, assert here. Otherwise this is a
    // code-review level check (the string literals in set_options_cf).
    assert!(store.is_bulk_sync_mode());
}
```

Since RocksDB doesn't expose a clean way to read back per-CF L0 options at runtime, this is primarily a code-review check. The test ensures the mode flag is set.

**Step 2: Run test to verify it fails (or passes as placeholder)**

Run:

```bash
cargo test -p ckbadger-store test_bulk_sync_l0_thresholds_are_raised -- --nocapture
```

Expected: May PASS already (mode flag test). The real change is the threshold values.

**Step 3: Write minimal implementation**

In `enter_bulk_sync_mode()` (around line 1490):

```rust
// BEFORE:
("level0_slowdown_writes_trigger", "64"),
("level0_stop_writes_trigger", "128"),

// AFTER:
("level0_slowdown_writes_trigger", "96"),
("level0_stop_writes_trigger", "192"),
```

Update the log message at line 1533:

```rust
// BEFORE:
"Bulk sync compaction options set: l0_slowdown=64, l0_stop=128, ..."
// AFTER:
"Bulk sync compaction options set: l0_slowdown=96, l0_stop=192, ..."
```

No changes needed in `restore_normal_compaction_options()` — it already restores to 12/24.

**Step 4: Run tests to verify they pass**

Run:

```bash
cargo test -p ckbadger-store -- --nocapture
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/ckbadger-store/src/store.rs
git commit -m "perf: raise bulk sync L0 thresholds (slowdown 64→96, stop 128→192)

Reduces writer stalls during bulk sync. Synergy with commit consolidation
(fewer commits = fewer flushes = slower L0 accumulation)."
```

---

### Task 6: Verify all tests pass and run cargo check + clippy

**Files:**

- No code changes

**Step 1: Run full test suite**

Run:

```bash
cargo check && cargo clippy
cargo test -p ckbadger-indexer --lib -- --nocapture
cargo test -p ckbadger-indexer reorg_handling -- --nocapture
cargo test -p ckbadger-store -- --nocapture
cargo test -p ckbadger-api -- --nocapture
```

Expected: All PASS with no warnings.

**Step 2: If failures, fix and re-run**

Any failures here indicate a regression from the refactoring. Fix the root cause in the relevant task's files, do not add workarounds.

---

### Task 7: Fresh-db bulk sync verification

**Files:**

- No code changes (verification only)

**Step 1: Run fresh-db sync**

Run:

```bash
ckbadger purge
ckbadger run
```

Wait for bulk sync to complete. Monitor logs for:

- Batch count should be ~3,800 (similar to best run)
- No pipeline errors or batch mismatches
- Bulk sync completes and transitions to live sync

**Step 2: Compare perf artifacts**

Check `temp/perf/bulk-sync/latest/report.md`:

- `wall_clock_seconds` should beat 4,774s (best run)
- `blocks_per_sec_wall` should exceed 3,935
- `batches` should be ~3,800 range
- `avg_commit_ms` should be substantially lower (commit consolidation effect)

**Step 3: Stop rule**

If wall clock does NOT beat 4,774s:

- Compare per-region throughput against best run samples
- Check if commit consolidation actually reduced commit time
- Check if controller produces expected batch distribution
- Do NOT proceed to further optimization without understanding the delta

**Step 4: Record results**

Update the design doc with actual before/after numbers.
