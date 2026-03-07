# Cells and Activities Write Amplification Reduction Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Restructure cell and activity storage so bulk sync writes far fewer hot-path mutations, especially on cell consume and multi-owner activity writes, while preserving exact inline index construction.

**Architecture:** Split immutable history from mutable canonical state. Cells become `append-only payload + append-only index + mutable state`; activities become `append-only tx envelope + append-only owner refs`, with canonical visibility enforced by read-time filtering against domain state.

**Tech Stack:** Rust, RocksDB (`ckbadger-store`), Tokio, Axum, inline Rust unit tests, integration tests in API routes

---

### Task 1: Add new CF constants, store-class assignments, and key encoders

**Files:**

- Modify: `crates/ckbadger-store/src/store.rs`
- Modify: `crates/ckbadger-store/src/keys.rs`
- Modify: `crates/ckbadger-store/src/lib.rs`
- Test: `crates/ckbadger-store/src/store.rs`
- Test: `crates/ckbadger-store/src/keys.rs`

**Step 1: Write the failing tests**

Add tests for:

```rust
#[test]
fn test_append_cfs_include_cell_payloads_and_activity_refs() {
    assert!(APPEND_CFS.contains(&CF_CELL_PAYLOADS));
    assert!(APPEND_CFS.contains(&CF_CELL_INDEX));
    assert!(APPEND_CFS.contains(&CF_ACTIVITY_TX_ENVELOPES));
    assert!(APPEND_CFS.contains(&CF_ACTIVITY_BY_OWNER));
}

#[test]
fn test_encode_decode_cell_payload_key_round_trips() {
    let tx_hash = vec![0x11; 32];
    let key = encode_cell_payload_key(123, &tx_hash, 0);
    let decoded = decode_cell_payload_key(&key).unwrap();
    assert_eq!(decoded.block_number, 123);
    assert_eq!(decoded.tx_hash, tx_hash);
    assert_eq!(decoded.output_index, 0);
}

#[test]
fn test_encode_decode_activity_owner_key_round_trips() {
    let lock = vec![0x22; 32];
    let tx_hash = vec![0x33; 32];
    let key = encode_activity_owner_key(&lock, 456, 7, &tx_hash);
    let decoded = decode_activity_owner_key(&key).unwrap();
    assert_eq!(decoded.lock_hash, lock);
    assert_eq!(decoded.block_number, 456);
    assert_eq!(decoded.tx_index, 7);
    assert_eq!(decoded.tx_hash, tx_hash);
}
```

**Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p ckbadger-store test_append_cfs_include_cell_payloads_and_activity_refs -- --nocapture
cargo test -p ckbadger-store test_encode_decode_cell_payload_key_round_trips -- --nocapture
cargo test -p ckbadger-store test_encode_decode_activity_owner_key_round_trips -- --nocapture
```

Expected: FAIL because the new CFs and key codecs do not exist yet.

**Step 3: Write the minimal implementation**

- Add CF constants:
  - `CF_CELL_PAYLOADS`
  - `CF_CELL_INDEX`
  - `CF_ACTIVITY_TX_ENVELOPES`
  - `CF_ACTIVITY_BY_OWNER`
- Put them in the correct store-class lists:
  - `APPEND_CFS`
  - `ALL_CFS`
  - `HIGH_WRITE_CFS` or `HISTORICAL_APPEND_CFS` where appropriate
- Add encode/decode helpers for:
  - `cell_payload_key`
  - `cell_index_key`
  - `activity_tx_envelope_key`
  - `activity_owner_key`

**Step 4: Run tests to verify they pass**

Run the same `cargo test` commands from Step 2.

Expected: PASS

**Step 5: Commit**

```bash
git add crates/ckbadger-store/src/store.rs crates/ckbadger-store/src/keys.rs crates/ckbadger-store/src/lib.rs
git commit -m "feat(store): add new cell and activity column families"
```

### Task 2: Add new store types and batch operations

**Files:**

- Modify: `crates/ckbadger-store/src/types.rs`
- Modify: `crates/ckbadger-store/src/batch.rs`
- Test: `crates/ckbadger-store/src/types.rs`
- Test: `crates/ckbadger-store/src/batch.rs`

**Step 1: Write the failing tests**

Add tests for:

```rust
#[test]
fn test_cell_state_serializes_live_and_consumed_forms() {
    let live = CellState::live(123, b"payload-key".to_vec());
    let bytes = bincode::serialize(&live).unwrap();
    let decoded: CellState = bincode::deserialize(&bytes).unwrap();
    assert!(decoded.is_live());

    let consumed = live.into_consumed(200, vec![0x44; 32]);
    let bytes = bincode::serialize(&consumed).unwrap();
    let decoded: CellState = bincode::deserialize(&bytes).unwrap();
    assert!(decoded.is_consumed());
}

#[test]
fn test_batch_puts_new_cell_payload_state_and_index() {
    let dir = tempfile::tempdir().unwrap();
    let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
    let mut batch = StoreBatch::new(&store);
    batch.put_cell_payload(123, &[0x11; 32], 0, &sample_live_cell_info());
    batch.put_cell_state(&[0x11; 32], 0, &CellState::live(123, b"p".to_vec()));
    batch.put_cell_index(IndexTag::Lock, &[0x22; 32], 123, &[0x11; 32], 0);
    batch.commit().unwrap();
}

#[test]
fn test_batch_puts_activity_envelope_and_owner_ref() {
    let dir = tempfile::tempdir().unwrap();
    let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
    let mut batch = StoreBatch::new(&store);
    batch.put_activity_tx_envelope(100, 1, &[0x55; 32], &sample_envelope());
    batch.put_activity_owner_ref(&[0x66; 32], 100, 1, &[0x55; 32], 0);
    batch.commit().unwrap();
}
```

**Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p ckbadger-store test_cell_state_serializes_live_and_consumed_forms -- --nocapture
cargo test -p ckbadger-store test_batch_puts_new_cell_payload_state_and_index -- --nocapture
cargo test -p ckbadger-store test_batch_puts_activity_envelope_and_owner_ref -- --nocapture
```

Expected: FAIL because the types and batch helpers do not exist yet.

**Step 3: Write the minimal implementation**

- Add:
  - `CellState`
  - `ActivityTxEnvelope`
  - `OwnerActivityViewStored`
- Add batch helpers:
  - `put_cell_payload`
  - `put_cell_state`
  - `put_activity_tx_envelope`
  - `put_activity_owner_ref`
  - `put_cell_index`

**Step 4: Run tests to verify they pass**

Run the same `cargo test` commands from Step 2.

Expected: PASS

**Step 5: Commit**

```bash
git add crates/ckbadger-store/src/types.rs crates/ckbadger-store/src/batch.rs
git commit -m "feat(store): add cell state and activity envelope types"
```

### Task 3: Switch cell read operations to the new schema

**Files:**

- Modify: `crates/ckbadger-store/src/cell_ops.rs`
- Test: `crates/ckbadger-store/src/cell_ops.rs`

**Step 1: Write the failing tests**

Add or replace focused tests for:

```rust
#[test]
fn test_get_cell_reads_live_state_then_payload() {
    let store = seed_store_with_new_cell_schema();
    let cell = store.get_cell(&TX_HASH, 0).unwrap().unwrap();
    assert_eq!(cell.created_at_block, 123);
}

#[test]
fn test_get_consumed_cell_info_reads_state_then_payload() {
    let store = seed_store_with_consumed_cell_schema();
    let info = store.get_consumed_cell_info(&TX_HASH, 0).unwrap().unwrap();
    assert_eq!(info.consumed_at_block, 200);
}

#[test]
fn test_list_cells_by_lock_skips_stale_historical_index_entries() {
    let store = seed_store_with_stale_cell_index_entries();
    let rows = store.list_cells_by_lock(&LOCK_HASH, 10, None).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, 0);
}
```

**Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p ckbadger-store test_get_cell_reads_live_state_then_payload -- --nocapture
cargo test -p ckbadger-store test_get_consumed_cell_info_reads_state_then_payload -- --nocapture
cargo test -p ckbadger-store test_list_cells_by_lock_skips_stale_historical_index_entries -- --nocapture
```

Expected: FAIL because readers still expect the old `cells/live_cells/consumed_cells` layout.

**Step 3: Write the minimal implementation**

- Rework:
  - `get_cell`
  - `get_cells_batch`
  - `get_consumed_cell`
  - `get_consumed_cell_info`
  - `get_consumed_cells_batch`
  - `list_cells_by_lock`
  - `list_cells_by_type`
  - `list_cells_by_lock_code_hash`
  - `list_cells_by_type_code_hash`
- Batch state lookups before payload lookups to avoid N+1 reads where possible.
- Enforce:
  - `state.is_live()` for live queries
  - `state.created_at_block == index.created_at_block` for index-backed visibility

**Step 4: Run tests to verify they pass**

Run the same `cargo test` commands from Step 2 plus the existing cell-op test module.

Expected: PASS

**Step 5: Commit**

```bash
git add crates/ckbadger-store/src/cell_ops.rs
git commit -m "feat(store): read cells from payload plus state schema"
```

### Task 4: Rewrite the cell write path in the indexer

**Files:**

- Modify: `crates/indexer/src/db/writer/cells.rs`
- Modify: `crates/indexer/src/sync/batch.rs`
- Modify: `crates/indexer/src/sync/undo.rs`
- Test: `crates/indexer/src/db/writer/cells.rs`
- Test: `crates/indexer/src/sync/undo.rs`

**Step 1: Write the failing tests**

Add regression tests for:

```rust
#[test]
fn test_insert_cells_batch_writes_payload_state_and_historical_indexes() {
    let (store, writer) = setup_new_schema_writer();
    writer.insert_cells_batch(&all_cells, &precomputed, &mut batch, true).unwrap();
    assert!(store.get_cell_state(&TX_HASH, 0).unwrap().unwrap().is_live());
}

#[test]
fn test_consume_cells_batch_updates_only_state_not_indexes() {
    let (store, writer) = setup_seeded_live_cell();
    writer.consume_cells_batch_preloaded(&consumptions, &preloaded, &same_batch, &mut batch, true).unwrap();
    let state = store.get_cell_state(&TX_HASH, 0).unwrap().unwrap();
    assert!(state.is_consumed());
    assert!(store.cell_index_entry_exists(IndexTag::Lock, &LOCK_HASH, CREATED_AT, &TX_HASH, 0).unwrap());
}

#[test]
fn test_rollback_restores_live_state_without_deleting_historical_index() {
    let store = seed_consumed_state_with_historical_index();
    rollback_to_before_consumption(&store);
    assert!(store.get_cell(&TX_HASH, 0).unwrap().is_some());
}
```

**Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p ckbadger-indexer test_insert_cells_batch_writes_payload_state_and_historical_indexes -- --nocapture
cargo test -p ckbadger-indexer test_consume_cells_batch_updates_only_state_not_indexes -- --nocapture
cargo test -p ckbadger-indexer test_rollback_restores_live_state_without_deleting_historical_index -- --nocapture
```

Expected: FAIL because the writer and undo path still mutate the old CF family.

**Step 3: Write the minimal implementation**

- Update cell insertion to:
  - append payload
  - put state live
  - append index entries
- Update cell consumption to:
  - mutate state only
- Remove cell index deletes from the hot path.
- Update undo helpers so rollback restores `cell_state` only.

**Step 4: Run tests to verify they pass**

Run the same `cargo test` commands from Step 2.

Expected: PASS

**Step 5: Commit**

```bash
git add crates/indexer/src/db/writer/cells.rs crates/indexer/src/sync/batch.rs crates/indexer/src/sync/undo.rs
git commit -m "refactor(indexer): move cells to state plus append-only history"
```

### Task 5: Add the normalized activity schema and query helpers

**Files:**

- Modify: `crates/ckbadger-store/src/activity_ops.rs`
- Modify: `crates/ckbadger-store/src/batch.rs`
- Modify: `crates/ckbadger-store/src/types.rs`
- Test: `crates/ckbadger-store/src/activity_ops.rs`
- Test: `crates/ckbadger-store/src/batch.rs`

**Step 1: Write the failing tests**

Add tests for:

```rust
#[test]
fn test_list_activities_reconstructs_owner_view_from_slot() {
    let store = seed_activity_owner_ref_and_envelope();
    let rows = store.list_activities(&LOCK_HASH, 10, None, None).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].2.tx_hash, TX_HASH.to_vec());
}

#[test]
fn test_list_activities_reconstructs_peers_from_participants() {
    let store = seed_multi_owner_activity();
    let rows = store.list_activities(&LOCK_A, 10, None, None).unwrap();
    assert_eq!(rows[0].2.peers, vec![LOCK_B.to_vec(), LOCK_C.to_vec()]);
}

#[test]
fn test_list_activities_skips_orphaned_owner_ref_when_canonical_location_mismatches() {
    let store = seed_orphaned_activity_history();
    let rows = store.list_activities(&LOCK_HASH, 10, None, None).unwrap();
    assert!(rows.is_empty());
}
```

**Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p ckbadger-store test_list_activities_reconstructs_owner_view_from_slot -- --nocapture
cargo test -p ckbadger-store test_list_activities_reconstructs_peers_from_participants -- --nocapture
cargo test -p ckbadger-store test_list_activities_skips_orphaned_owner_ref_when_canonical_location_mismatches -- --nocapture
```

Expected: FAIL because activity reads still expect full per-owner rows.

**Step 3: Write the minimal implementation**

- Add envelope/owner-ref read helpers.
- Rebuild `list_activities()` as:
  - scan owner refs
  - fetch envelopes
  - project owner slot
  - reconstruct peers from participants
  - apply filter

**Step 4: Run tests to verify they pass**

Run the same `cargo test` commands from Step 2.

Expected: PASS

**Step 5: Commit**

```bash
git add crates/ckbadger-store/src/activity_ops.rs crates/ckbadger-store/src/batch.rs crates/ckbadger-store/src/types.rs
git commit -m "feat(store): normalize activity storage by tx envelope"
```

### Task 6: Rewrite activity generation and API usage

**Files:**

- Modify: `crates/indexer/src/db/writer/activities.rs`
- Modify: `crates/indexer/src/sync/batch.rs`
- Modify: `crates/api/src/routes/activities.rs`
- Test: `crates/indexer/src/db/writer/activities.rs`
- Test: `crates/api/src/routes/activities.rs`

**Step 1: Write the failing tests**

Add tests for:

```rust
#[test]
fn test_build_activities_for_block_emits_one_envelope_and_n_owner_refs() {
    let built = build_normalized_activities_for_block(&tx_views, &token_cache).unwrap();
    assert_eq!(built.envelopes.len(), 1);
    assert_eq!(built.owner_refs.len(), 3);
}

#[tokio::test]
async fn test_list_canonical_activities_page_uses_append_only_activity_store() {
    let state = seed_api_state_with_new_activity_schema();
    let page = list_canonical_activities_page(
        state.append_only_store.as_ref(),
        state.store.as_ref(),
        &LOCK_HASH,
        10,
        None,
        None,
    )
    .unwrap();
    assert_eq!(page.len(), 1);
}
```

**Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p ckbadger-indexer test_build_activities_for_block_emits_one_envelope_and_n_owner_refs -- --nocapture
cargo test -p ckbadger-api test_list_canonical_activities_page_uses_append_only_activity_store -- --nocapture
```

Expected: FAIL because the indexer still emits full `ActivityEntry` rows.

**Step 3: Write the minimal implementation**

- Replace the activity builder output with:
  - one envelope per tx
  - one owner ref per owner
- Update the writer hot path to append normalized activity rows.
- Update the API route to rely on the new append-only reader path without changing the external response shape.

**Step 4: Run tests to verify they pass**

Run the same `cargo test` commands from Step 2.

Expected: PASS

**Step 5: Commit**

```bash
git add crates/indexer/src/db/writer/activities.rs crates/indexer/src/sync/batch.rs crates/api/src/routes/activities.rs
git commit -m "refactor(activities): store tx envelopes and owner refs"
```

### Task 7: Remove old CF usage and add schema/regression verification

**Files:**

- Modify: `docs/STORE_SCHEMA.md`
- Modify: `crates/ckbadger-store/src/store.rs`
- Modify: any remaining references found by ripgrep
- Test: focused tests in touched files

**Step 1: Write the failing tests**

Add or update tests that assert:

```rust
#[test]
fn test_old_activity_cf_is_not_in_store_layout() {
    assert!(!ALL_CFS.contains(&CF_ACTIVITIES));
}

#[test]
fn test_old_live_and_consumed_cell_cfs_are_not_in_store_layout() {
    assert!(!ALL_CFS.contains(&CF_LIVE_CELLS));
    assert!(!ALL_CFS.contains(&CF_CONSUMED_CELLS));
}
```

**Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p ckbadger-store test_old_activity_cf_is_not_in_store_layout -- --nocapture
cargo test -p ckbadger-store test_old_live_and_consumed_cell_cfs_are_not_in_store_layout -- --nocapture
```

Expected: FAIL because the old schema constants and CF lists still exist.

**Step 3: Write the minimal implementation**

- Remove old CFs from the active schema.
- Update schema docs.
- Remove dead code that only supports the old cell/activity layout.

**Step 4: Run tests to verify they pass**

Run the same `cargo test` commands from Step 2 plus the focused cell/activity store tests.

Expected: PASS

**Step 5: Commit**

```bash
git add docs/STORE_SCHEMA.md crates/ckbadger-store/src/store.rs
git commit -m "refactor(store): remove legacy cell and activity schema"
```

### Task 8: Run focused performance and correctness verification

**Files:**

- No new files required unless benchmark notes are captured

**Step 1: Run the focused Rust test suites**

Run:

```bash
cargo test -p ckbadger-store cell_ops -- --nocapture
cargo test -p ckbadger-store activity_ops -- --nocapture
cargo test -p ckbadger-indexer writer::cells -- --nocapture
cargo test -p ckbadger-indexer writer::activities -- --nocapture
cargo test -p ckbadger-api activities -- --nocapture
```

Expected: PASS

**Step 2: Run full crate checks**

Run:

```bash
cargo check
cargo test -p ckbadger-store
cargo test -p ckbadger-indexer
cargo test -p ckbadger-api
```

Expected: PASS

**Step 3: Rebuild and capture perf comparison**

Run the fresh-db bulk sync workflow and compare against the current perf baseline.

Record:

- `avg_commit_ms`
- `p99_commit_ms`
- `t1_ms`
- `t_act_ms`
- `max_compaction_pending_mb`
- cell/activity CF cumulative compaction write GB

Expected:

- materially lower `T1`
- materially lower activity write bytes
- exact query correctness preserved

**Step 4: Commit**

```bash
git commit --allow-empty -m "chore: verify cell and activity write amplification reduction"
```
