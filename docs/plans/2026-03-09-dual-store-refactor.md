# Dual-Store Boundary Refactor Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Simplify dual-store design so CF_CELLS is the only append-only CF; all other CFs (including activities, addr_txs) move to domain store. Fix missing `type_hash_type` in LiveCellInfo.

**Architecture:** Two RocksDB stores remain (domain + append-only), but the boundary changes. Append-only store shrinks from 4 CFs to 1 (CF_CELLS only). The 4 activity/history CFs move to domain, gaining proper reorg rollback (range delete) instead of ghost-entry filtering. Cell reads become cross-store: live-cell markers in domain, cell payloads in append-only.

**Tech Stack:** Rust, RocksDB, ckbadger-store crate, ckbadger-indexer crate, ckbadger-api crate

**Breaking change:** Requires full DB rebuild after merge (delete data/domain + data/append-only, re-sync).

---

## Task 1: Add `type_hash_type` to LiveCellInfo

Independent fix. No store boundary dependency.

**Files:**

- Modify: `crates/ckbadger-store/src/types.rs:15-31`
- Modify: `crates/indexer/src/parser/cell.rs` (populate new field)
- Modify: `crates/indexer/src/db/writer/cells.rs` (if LiveCellInfo construction happens here)
- Test: `crates/ckbadger-store/src/cell_ops.rs` (existing tests use `make_cell` helper)

**Step 1: Add field to LiveCellInfo**

In `crates/ckbadger-store/src/types.rs`, add `type_hash_type` after `type_code_hash`:

```rust
pub struct LiveCellInfo {
    pub capacity: i64,
    pub created_at_block: i64,
    pub lock_script_hash: Vec<u8>,
    pub lock_code_hash: Vec<u8>,
    pub lock_hash_type: i16,
    pub lock_args: Vec<u8>,
    pub type_script_hash: Option<Vec<u8>>,
    pub type_code_hash: Option<Vec<u8>>,
    #[serde(default)]
    pub type_hash_type: Option<i16>,   // <-- NEW
    #[serde(default)]
    pub type_args: Option<Vec<u8>>,
    pub data_size: i32,
    #[serde(default)]
    pub occupied_capacity: i64,
    #[serde(default)]
    pub udt_amount: Option<u128>,
}
```

Use `#[serde(default)]` so existing serialized data deserializes without breaking (field defaults to `None`).

**Step 2: Populate in cell parser**

Find where `LiveCellInfo` is constructed in `crates/indexer/src/parser/cell.rs`. Add `type_hash_type` field using `ScriptParser::hash_type_to_i16(&type_script.hash_type)` when type_script is present, `None` otherwise. Pattern matches the existing `type_code_hash` population.

**Step 3: Update test helpers**

In `crates/ckbadger-store/src/cell_ops.rs`, update `make_cell()` test helper to include `type_hash_type: Some(1)` when type_script is present.

**Step 4: Run tests**

```bash
cargo test -p ckbadger-store -- cell
cargo test -p ckbadger-indexer -- cell
```

**Step 5: Commit**

```bash
git add crates/ckbadger-store/src/types.rs crates/indexer/src/parser/cell.rs crates/ckbadger-store/src/cell_ops.rs
git commit -m "fix(store): add type_hash_type to LiveCellInfo"
```

---

## Task 2: Update Store Schema Constants

Move CFs between DOMAIN_CFS and APPEND_CFS.

**Files:**

- Modify: `crates/ckbadger-store/src/store.rs:375-422` (DOMAIN_CFS, APPEND_CFS)
- Modify: `crates/ckbadger-store/src/store.rs:908-913` (HISTORICAL_APPEND_CFS)
- Modify: `crates/ckbadger-store/src/store.rs:340-372` (ALL_CFS)
- Modify: `crates/ckbadger-store/src/lib.rs` (re-exports)

**Step 1: Update APPEND_CFS**

```rust
/// Column families for the append-only store (immutable, hash-keyed facts).
pub const APPEND_CFS: &[&str] = &[
    CF_CELLS,
];
```

**Step 2: Update DOMAIN_CFS**

Add the 4 CFs that were in APPEND_CFS. Remove CF_CELLS.

```rust
pub const DOMAIN_CFS: &[&str] = &[
    // CF_CELLS removed — now in APPEND_CFS
    CF_LIVE_CELLS,
    CF_CONSUMED_CELLS,
    CF_REORG_UNDO_LOG_BY_BLOCK,
    CF_BLOCK_HEADERS,
    CF_BLOCK_HASH_INDEX,
    CF_CELL_BY_LOCK,
    CF_CELL_BY_TYPE,
    CF_CELL_BY_LOCK_CODE,
    CF_CELL_BY_TYPE_CODE,
    CF_TX_INDEX,
    CF_TX_HASH_MAP,
    CF_ADDR_BALANCE,
    CF_ADDR_TXS,             // <-- moved from APPEND_CFS
    CF_DAO_DEPOSITS,
    CF_DAO_BY_WITHDRAW_TX,
    CF_DAO_BY_BLOCK,
    CF_DAO_BY_LOCK_BLOCK,
    CF_DAO_BY_STATUS_BLOCK,
    CF_TOKENS,
    CF_TOKEN_HOLDERS,
    CF_SPORE_DATA,
    CF_OBJECT_DATA,
    CF_OBJECT_BY_COLLECTION,
    CF_IDENTITY_DATA,
    CF_STATS_CHAIN,
    CF_STATS_DAO,
    CF_STATS_HODL,
    CF_STATS_SCRIPT,
    CF_STATS_TOKEN,
    CF_STATS_SPORE,
    CF_STATS_OBJECT,
    CF_SCRIPT_INFO,
    CF_SYNC_META,
    CF_SPORE_BY_CLUSTER,
    CF_TOKEN_TRANSFERS,
    CF_ACTIVITIES,                         // <-- moved from APPEND_CFS
    CF_CLUSTER_AGG,
    CF_OBJECT_COLLECTION_AGG,
    CF_OBJECT_COLLECTION_ACTIVITIES,       // <-- moved from APPEND_CFS
    CF_IDENTITY_AGG,
    CF_IDENTITY_COLLECTION_ACTIVITIES,     // <-- moved from APPEND_CFS
];
```

**Step 3: Update HISTORICAL_APPEND_CFS**

These CFs used universal compaction. Now only CF_CELLS qualifies:

```rust
const HISTORICAL_APPEND_CFS: &'static [&'static str] = &[
    CF_CELLS,
];
```

The 4 activity CFs moving to domain will use the domain store's default compaction (leveled). CF_ACTIVITIES should be added to HIGH_WRITE_CFS if not already there (check existing membership).

**Step 4: Verify ALL_CFS still contains all 42 CFs**

ALL_CFS (used by TestUnified) must include all CFs from both stores. Verify CF_CELLS is still present.

**Step 5: Run compile check**

```bash
cargo check -p ckbadger-store
```

This will surface panics from `cf()` accessor calls where CFs are now in the wrong store. Fix any `cf_cells()` calls that assume domain store — these will need updating in later tasks.

**Step 6: Commit**

```bash
git commit -m "refactor(store): move activity CFs to domain, CF_CELLS to append-only"
```

---

## Task 3: Update Append-Only Validation Logic

Simplify validation to only apply to CF_CELLS.

**Files:**

- Modify: `crates/ckbadger-store/src/store.rs:520-593` (is*append_only, append_cf_name_for_handle, validate*\*)

**Step 1: Simplify `append_cf_name_for_handle`**

```rust
pub(crate) fn append_cf_name_for_handle(
    &self,
    cf: &ColumnFamily,
) -> anyhow::Result<&'static str> {
    if !self.is_append_only_store() {
        anyhow::bail!(
            "append_cf_name_for_handle called on non-append store: {:?}",
            self.store_class
        );
    }
    if std::ptr::eq(cf, self.cf_cells()) {
        return Ok(CF_CELLS);
    }
    anyhow::bail!(
        "unknown append-only column family handle in {:?} store",
        self.store_class
    );
}
```

**Step 2: Update validation functions**

`validate_append_put_by_cf_name` and `validate_append_delete_by_cf_name` remain unchanged in logic — they already check `is_append_only_store()` and work generically. No code change needed.

**Step 3: Run tests**

```bash
cargo test -p ckbadger-store
```

Expect failures in append-only batch tests that test CF_ACTIVITIES, CF_ADDR_TXS, etc. on append-only stores. These tests need updating in Task 10.

**Step 4: Commit**

```bash
git commit -m "refactor(store): simplify append-only validation to CF_CELLS only"
```

---

## Task 4: Refactor StoreBatch for New CF Boundaries

Update `commit_inner`, `put_cf`, `delete_cf` in batch.rs so that only CF_CELLS gets append-only treatment.

**Files:**

- Modify: `crates/ckbadger-store/src/batch.rs:127-230` (commit_inner, put_cf, delete_cf)
- Modify: `crates/ckbadger-store/src/batch.rs:463-466` (put_addr_tx)
- Modify: `crates/ckbadger-store/src/batch.rs:778-833` (put*activity, put*\*\_collection_activity)

**Step 1: Update commit_inner**

No structural change needed — `commit_inner` already checks `is_append_only_store()` generically. Since the append-only store now only has CF_CELLS, the dedup/validation logic in `commit_inner` will only run for CF_CELLS writes. Works as-is.

**Step 2: Verify put_cf / delete_cf**

`put_cf` at line 190 checks `is_append_only_store()` and accumulates `append_ops`. Since put_addr_tx, put_activity, etc. now write to domain store batches (not append-only), their `put_cf` calls won't trigger append_ops accumulation. Works as-is.

The only method that will now trigger append-only handling is `put_cell` (and `put_cell_raw_key`), because it writes to CF_CELLS on the append-only store.

**Step 3: Verify put_cell already uses put_cf**

Check `batch.rs` for `put_cell` — ensure it calls `self.put_cf(self.store.cf_cells(), ...)`. If so, it will automatically get append-only validation when the store is append-only.

```bash
cargo check -p ckbadger-store
```

**Step 4: Commit**

```bash
git commit -m "refactor(store): verify StoreBatch handles new CF boundaries correctly"
```

---

## Task 5: Cross-Store Cell Reads

CF_CELLS now lives in append-only store but live/consumed markers and indexes stay in domain. Cell read methods need both stores.

**Files:**

- Modify: `crates/ckbadger-store/src/cell_ops.rs` (all public methods)
- Modify: `crates/ckbadger-store/src/reorg_ops.rs` (cell rollback reads)

**Step 1: Add `cells_store` parameter to cross-store methods**

Methods that need BOTH CF_LIVE_CELLS (domain) and CF_CELLS (append-only):

```rust
impl CkbadgerStore {
    /// Read cell payload from the cells store (append-only).
    /// Call on whichever store has CF_CELLS.
    pub fn get_cell_by_outpoint_key(
        &self,
        outpoint_key: &[u8],
    ) -> anyhow::Result<Option<LiveCellInfo>> {
        // unchanged — reads CF_CELLS from self
    }

    /// Check live status (domain) + read payload (cells_store).
    pub fn get_live_cell_by_outpoint_key(
        &self,
        outpoint_key: &[u8],
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<Option<LiveCellInfo>> {
        if self.get_cf(self.cf_live_cells(), outpoint_key)?.is_none() {
            return Ok(None);
        }
        cells_store.get_cell_by_outpoint_key(outpoint_key)
    }

    pub fn get_cell(
        &self,
        tx_hash: &[u8],
        output_index: i16,
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<Option<LiveCellInfo>> {
        let key = keys::encode_outpoint(tx_hash, output_index);
        self.get_live_cell_by_outpoint_key(&key, cells_store)
    }

    pub fn get_cells_batch(
        &self,
        outpoints: &[(&[u8], i16)],
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<HashMap<(Vec<u8>, i16), LiveCellInfo>> {
        // Check CF_LIVE_CELLS on self (domain), read CF_CELLS from cells_store
        // Same multi-get pattern but split across stores
    }

    pub fn get_consumed_cell_info(
        &self,
        tx_hash: &[u8],
        output_index: i16,
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<Option<ConsumedCellInfo>> {
        // Check CF_CONSUMED_CELLS on self (domain), read CF_CELLS from cells_store
    }

    // Similarly for get_consumed_cells_batch, get_consumed_cell_meta_batch,
    // list_cells_by_lock, list_cells_by_type, etc.
}
```

**Note for TestUnified:** When `store_class == TestUnified`, `self` has ALL CFs. Pass `self` as both domain and cells_store. Add a convenience method:

```rust
/// For test stores that have all CFs in one instance.
pub fn get_cell_unified(&self, tx_hash: &[u8], output_index: i16) -> anyhow::Result<Option<LiveCellInfo>> {
    self.get_cell(tx_hash, output_index, self)
}
```

**Step 2: Update all callers**

Callers in the codebase that call `store.get_cell(...)`:

- `crates/api/src/routes/cells.rs` — pass `state.append_only_store`
- `crates/api/src/routes/graph.rs` — pass `state.append_only_store`
- `crates/api/src/routes/search.rs` — pass `state.append_only_store`
- `crates/api/src/routes/transactions.rs` — pass `state.append_only_store`
- `crates/api/src/routes/statistics.rs` — pass `state.append_only_store`
- `crates/indexer/src/db/writer/cells.rs` — pass append_only_store reference
- `crates/indexer/src/verify/` — pass append_only_store reference
- `crates/ckbadger-store/src/reorg_ops.rs` — rollback already receives `tx_index_store` param (rename to `append_store`)

**Step 3: Update cell_ops.rs test helpers**

Test helpers using `make_cell` + `insert_cell` write to a TestUnified store. Update to pass `&store` as cells_store (self-reference works for unified stores).

**Step 4: Run tests**

```bash
cargo test -p ckbadger-store -- cell
```

**Step 5: Commit**

```bash
git commit -m "refactor(store): cross-store cell reads (domain markers + append-only payload)"
```

---

## Task 6: Update Indexer Bulk Sync Write Path

Change pipeline threads so activities/addr_txs write to domain store, CF_CELLS writes to append-only store.

**Files:**

- Modify: `crates/indexer/src/sync/batch.rs` — pipeline thread scope (lines 3785-4928)

**Step 1: T1 cells — write to append-only store**

Currently T1 creates `StoreBatch::new(store)` (domain) for cell writes. Change to write CF_CELLS to append-only:

```rust
// T1: cells — CF_CELLS goes to append-only, CF_LIVE_CELLS/CF_CONSUMED_CELLS stay domain
let h1 = s.spawn(move || -> Result<(f64, f64)> {
    let t = Instant::now();
    let mut domain_batch = StoreBatch::new(store);           // live_cells, consumed_cells
    let mut cells_batch = StoreBatch::new(append_only_store); // CF_CELLS
    // ... cell insertion: put_cell → cells_batch, put_live_cell → domain_batch
    // ... cell consumption: delete_live_cell → domain_batch, put_consumed_cell → domain_batch
    let commit_ms = commit_phase_no_wal("T1_cells_domain", first_block, last_block, domain_batch)?;
    let cells_commit_ms = commit_phase_no_wal("T1_cells_append", first_block, last_block, cells_batch)?;
    Ok((t.elapsed().as_secs_f64() * 1000.0, commit_ms + cells_commit_ms))
});
```

This requires `insert_cells_batch` and `consume_cells_batch` in `crates/indexer/src/db/writer/cells.rs` to accept separate batches for domain and cells stores.

**Step 2: T2 addr_txs — write to domain store (not append-only)**

Currently T2 creates `StoreBatch::new(append_only_store)` for addr_txs. Change:

```rust
// T2: Previously had separate append_history_batch. Now all domain.
let mut batch = StoreBatch::new(store);
// put_addr_tx → batch (domain store)
// No separate append commit needed
```

**Step 3: T6a/T6b activities — write to domain store**

Currently T6a creates `StoreBatch::new(append_only_store)` for collection activities. Change to `StoreBatch::new(store)`.

```rust
// T6a: spore
let mut batch = StoreBatch::new(store);
let mut activity_batch = StoreBatch::new(store);           // was: append_only_store
let mut identity_activity_batch = StoreBatch::new(store);  // was: append_only_store
```

Same for T6b.

**Step 4: T_ACT activities — write to domain store**

```rust
// T_ACT: activities
let mut activity_batch = StoreBatch::new(store);  // was: append_only_store
```

**Step 5: Update input cell prefetch to use append-only store**

Bulk sync prefetches input cell info for consumed cells. Currently reads from `store` (domain). Change to read from `append_only_store`:

```rust
// In prefetch section, calls like:
// writer.get_cells_info_batch(&outpoints) → needs append_only_store for CF_CELLS
```

The writer's cell batch methods need the `cells_store` parameter added in Task 5.

**Step 6: Run compile check**

```bash
cargo check -p ckbadger-indexer
```

Fix all compilation errors from changed store references.

**Step 7: Commit**

```bash
git commit -m "refactor(indexer): update bulk sync write path for new store boundaries"
```

---

## Task 7: Update Indexer Live Sync Write Path

Same changes as Task 6 but for the serial live sync path.

**Files:**

- Modify: `crates/indexer/src/sync/batch.rs` — live sync section (lines 4930-6461)

**Step 1: Change activity batch stores**

```rust
// Live sync batches — all domain now:
let mut data_batch = StoreBatch::new(self.writer.store());
let mut object_activity_batch = StoreBatch::new(self.writer.store());       // was: append_only_store
let mut identity_activity_batch = StoreBatch::new(self.writer.store());     // was: append_only_store
let mut append_history_batch = StoreBatch::new(self.writer.store());        // was: append_only_store (addr_txs)
let mut activity_batch = StoreBatch::new(self.writer.store());              // was: append_only_store
```

**Step 2: Change cell writes**

Cell writes in live sync path need separate domain + append-only batches, same pattern as Task 6 Step 1.

**Step 3: Run compile check**

```bash
cargo check -p ckbadger-indexer
```

**Step 4: Commit**

```bash
git commit -m "refactor(indexer): update live sync write path for new store boundaries"
```

---

## Task 8: Simplify Reorg Rollback for Activity CFs

Now that activities are in domain store, rollback can delete them directly instead of leaving ghost entries.

**Files:**

- Modify: `crates/ckbadger-store/src/reorg_ops.rs:520-1770`

**Step 1: Add range-delete for CF_ACTIVITIES**

After deleting block_headers and tx entries (existing stages), add:

```rust
// Delete activities for blocks > rollback_to
// Activity key: lock_hash(32) + block_num_desc(8) + tx_idx_desc(4) + block_hash(32) + tx_hash(32)
// Since block_num is descending in key, entries for higher blocks sort BEFORE lower blocks.
// Iterate from start, delete until block_num <= rollback_to.
let mut stage = RollbackStageProgress::new("delete_activities");
let mut activities_removed = 0u64;
let iter = self.iterator_cf(self.cf_activities(), IteratorMode::Start);
for item in iter {
    let (key, _) = item?;
    if key.len() != keys::ACTIVITY_KEY_SIZE { continue; }
    let (_, block_num, _, _, _) = keys::decode_activity_key(&key);
    if block_num <= rollback_to { continue; }  // NOTE: descending keys — can't break early
    batch.delete_cf(self.cf_activities(), &key);
    activities_removed += 1;
    stage.tick(activities_removed);
}
stage.finish(activities_removed);
```

**Important**: Activity keys use descending block_num, so entries for block 1000 sort before block 999. For a shallow rollback (~36 blocks), iterating from start and filtering is fine. For optimization, could seek to the descending block_num range, but simple iteration is correct for shallow reorgs.

**Step 2: Add range-delete for CF_ADDR_TXS**

Same pattern. addr_txs key: lock_hash(32) + block_num_desc(8) + tx_idx_desc(4) + tx_hash(32).

```rust
let mut stage = RollbackStageProgress::new("delete_addr_txs");
let mut addr_txs_removed = 0u64;
let iter = self.iterator_cf(self.cf_addr_txs(), IteratorMode::Start);
for item in iter {
    let (key, _) = item?;
    if key.len() != keys::ADDR_TX_KEY_SIZE { continue; }
    let (_, block_num, _, _) = keys::decode_addr_tx_key(&key);
    if block_num <= rollback_to { continue; }
    batch.delete_cf(self.cf_addr_txs(), &key);
    addr_txs_removed += 1;
    stage.tick(addr_txs_removed);
}
stage.finish(addr_txs_removed);
```

**Step 3: Add range-delete for CF_OBJECT_COLLECTION_ACTIVITIES + CF_IDENTITY_COLLECTION_ACTIVITIES**

Same pattern with `keys::decode_nft_collection_activity_key`.

**Step 4: Remove ghost-entry filtering from aggregate repair**

In the existing collection aggregate repair (lines 1651-1756), remove the canonical-filtering logic. Now that orphaned entries are deleted, all remaining entries are canonical. Replace with a simple count:

```rust
let mut nft_activity_totals: HashMap<Vec<u8>, i64> = HashMap::new();
let iter = self.iterator_cf(
    self.cf_object_collection_activities(),
    IteratorMode::Start,
);
for item in iter {
    let (key, _) = item?;
    if key.len() != keys::NFT_COLLECTION_ACTIVITY_KEY_SIZE { continue; }
    let (collection_id, _, _, _, _) = keys::decode_nft_collection_activity_key(&key);
    *nft_activity_totals.entry(collection_id.to_vec()).or_insert(0) += 1;
}
```

No more `get_tx_location` + canonical verification per entry.

**Step 5: Rename `tx_index_store` parameter**

In `rollback_to_block_with_tx_index_store`, the `tx_index_store: Option<&CkbadgerStore>` parameter was the append-only store (for reading activities). Now activities are in domain, but CF_CELLS is in append-only. Rename to `cells_store` for clarity:

```rust
pub fn rollback_to_block_with_cells_store(
    &self,
    rollback_to: i64,
    cells_store: Option<&CkbadgerStore>,
) -> anyhow::Result<RollbackResult>
```

**Step 6: Run tests**

```bash
cargo test -p ckbadger-store -- rollback
cargo test -p ckbadger-store -- reorg
```

**Step 7: Commit**

```bash
git commit -m "refactor(store): simplify reorg rollback — delete activity entries directly"
```

---

## Task 9: Update Undo Log Handling

Activity undo entries no longer need AppendOnly target since they're in domain.

**Files:**

- Modify: `crates/indexer/src/sync/undo.rs`
- Modify: `crates/ckbadger-store/src/undo_log_ops.rs`

**Step 1: Remove append-delete undo entries for activities**

In `crates/indexer/src/sync/undo.rs`, the functions `put_addr_tx_with_undo_log`, `put_activity_with_undo_log`, and `put_append_delete_undo_entry` recorded AppendOnly target undo entries. Since activities are now in domain:

- `put_addr_tx_with_undo_log` → addr_txs are now domain writes. Undo entry should be a domain delete (or simply rely on the range-delete rollback from Task 8). Simplify to not record undo entries for these — the rollback range-delete handles cleanup.
- `put_activity_with_undo_log` → same, simplify.
- `put_append_delete_undo_entry` → only needed for actual append-only operations. If nothing uses it after removing activity entries, delete it. If CF_CELLS needs undo entries for reorg, keep it (but CF_CELLS shouldn't need undo entries — orphaned cells are harmless dead data).

**Step 2: Clean up AppendOnly undo log target**

In `crates/ckbadger-store/src/undo_log_ops.rs`, the `UndoLogStoreTarget::AppendOnly` variant was a no-op during rollback replay:

```rust
UndoLogStoreTarget::AppendOnly => {
    // Append-only store is immutable after write.
    // Reorg replay only prunes the undo-log entry.
}
```

Since no new AppendOnly undo entries will be created (activities are domain, CF_CELLS doesn't need undo), this variant can be kept for backward compat but won't be actively used.

**Step 3: Run tests**

```bash
cargo test -p ckbadger-indexer -- undo
cargo test -p ckbadger-store -- undo
```

**Step 4: Commit**

```bash
git commit -m "refactor(indexer): remove append-only undo log entries for domain activity CFs"
```

---

## Task 10: Update API Read Paths

Activity/addr_txs reads now come from domain store. Cell reads need append-only store for CF_CELLS.

**Files:**

- Modify: `crates/api/src/routes/activities.rs`
- Modify: `crates/api/src/routes/cells.rs`
- Modify: `crates/api/src/routes/assets.rs`
- Modify: `crates/api/src/routes/spore.rs`
- Modify: `crates/api/src/routes/graph.rs`
- Modify: `crates/api/src/routes/search.rs`
- Modify: `crates/api/src/routes/transactions.rs`
- Modify: `crates/api/src/routes/statistics.rs`

**Step 1: Activity/addr_txs reads → domain store**

API handlers that currently read from `state.append_only_store` for activities and addr_txs should switch to `state.store`:

```rust
// Before:
let activities = state.append_only_store.list_activities(&lock_hash, limit, cursor)?;
// After:
let activities = state.store.list_activities(&lock_hash, limit, cursor)?;
```

Same pattern for `list_addr_txs_recent`, `list_object_collection_activities`, `list_identity_collection_activities`.

**Step 2: Cell reads → pass append-only store**

Cell endpoints that call `state.store.get_cell(...)` need the extra `cells_store` parameter:

```rust
// Before:
let cell = state.store.get_cell(&tx_hash, output_index)?;
// After:
let cell = state.store.get_cell(&tx_hash, output_index, &state.append_only_store)?;
```

**Step 3: Verify API still opens both stores**

`crates/api/src/entry.rs` must still open both domain (secondary) and append-only (secondary) stores. The append-only store is still needed for CF_CELLS reads.

**Step 4: Run API integration tests**

```bash
cargo test -p ckbadger-api
```

**Step 5: Commit**

```bash
git commit -m "refactor(api): update read paths for new store boundaries"
```

---

## Task 11: Update Batch Tests

Fix test failures from changed CF assignments.

**Files:**

- Modify: `crates/ckbadger-store/src/batch.rs` (test module, lines 1073-2190)
- Modify: `crates/indexer/tests/reorg_handling.rs`
- Modify: `crates/api/tests/api_integration.rs`

**Step 1: Fix append-only batch tests**

Tests that create `open_append_only()` stores and test CF_ACTIVITIES, CF_ADDR_TXS validation should be rewritten to test CF_CELLS validation instead:

```rust
#[test]
fn test_append_only_batch_rejects_duplicate_cell_key() {
    let dir = TempDir::new().unwrap();
    let store = CkbadgerStore::open_append_only(dir.path()).unwrap();
    let mut batch = StoreBatch::new(&store);
    let cell = make_test_live_cell_info();
    batch.put_cell(&[0xAA; 32], 0, &cell);
    batch.commit().unwrap();

    let mut batch2 = StoreBatch::new(&store);
    batch2.put_cell(&[0xAA; 32], 0, &cell);
    // Should fail: duplicate key in append-only
    assert!(batch2.commit().is_err());
}
```

**Step 2: Fix activity tests that assumed append-only store**

Tests for `put_activity`, `put_object_collection_activity`, etc. that used `open_append_only()` should switch to `open_test_unified()` or domain stores.

**Step 3: Fix reorg handling tests**

Tests in `crates/indexer/tests/reorg_handling.rs` that verify ghost entries remain after rollback should verify entries are DELETED instead.

**Step 4: Run full test suite**

```bash
cargo test
cd frontend && pnpm test
```

**Step 5: Commit**

```bash
git commit -m "test: update tests for new dual-store boundaries"
```

---

## Task 12: Update Documentation

**Files:**

- Modify: `CLAUDE.md`
- Modify: `README.md`
- Modify: `docs/STORE_SCHEMA.md`
- Modify: `docs/prompts/ACTIVITY_SYSTEM.md`
- Modify: `docs/prompts/REORG_HANDLING.md`

**Step 1: Update CLAUDE.md**

Update the DB Responsibility Boundary section:

```markdown
## DB Responsibility Boundary (MANDATORY)

- **Indexer owns all RocksDB writes**
- **API is read-only for RocksDB**
- **Domain store responsibility**: domain store (`[store].domain_data_path`) is the mutable canonical/query state. All CFs except CF_CELLS live here. May perform create/update/delete as required by chain progression and reorg handling, but only via indexer.
- **Append-only store responsibility**: append-only store (`[store].append_only_data_path`) stores CF_CELLS only — immutable cell payloads keyed by outpoint (tx_hash + output_index). Write-once, never updated or deleted. Cell payloads are content-addressed facts that remain valid regardless of fork.
- **Append-only correction policy**: if append-only cell data is wrong, fix indexer logic and rebuild from genesis; do not patch with in-place update/delete.
```

Update Store Boundary Check Rules:

```markdown
## Store Boundary Check Rules (MANDATORY)

- CF_CELLS is the only append-only CF. All other CFs are domain (canonical view).
- Any write path to CF_CELLS must enforce append-only semantics: new-key only, no update, no delete.
- Cell reads are cross-store: live/consumed markers in domain, cell payloads in append-only.
```

Update CF count references: "dual-store, 42 canonical CFs" → keep count, clarify "41 domain + 1 append-only".

**Step 2: Update README.md**

Update storage description to reflect simplified boundary.

**Step 3: Update docs/STORE_SCHEMA.md**

Rewrite header:

```markdown
ckbadger runs two logical RocksDB stores:

- **Domain store** (`[store].domain_data_path`) — canonical chain view (41 CFs). All mutable state including activities, address history, indexes, and aggregates.
- **Append-only store** (`[store].append_only_data_path`) — immutable cell payloads (1 CF: `cells`). Content-addressed by outpoint, write-once, never deleted even during reorg.
```

Mark `cells` row in CF table as "(append-only store)". All others are domain.

**Step 4: Update docs/prompts/ACTIVITY_SYSTEM.md**

Remove references to append-only storage for activities. Update rollback section:

```markdown
### Rollback

Activity entries are in the domain store. During reorg rollback, entries for blocks > fork_point are deleted directly via range scan. No ghost entries, no canonical filtering.
```

**Step 5: Update docs/prompts/REORG_HANDLING.md**

Replace lines 44-45:

```markdown
4. Deletes activity entries (CF_ACTIVITIES, CF_ADDR_TXS, CF_OBJECT_COLLECTION_ACTIVITIES, CF_IDENTITY_COLLECTION_ACTIVITIES) for rolled-back blocks
5. Rebuilds addr_balance and collection aggregates from remaining canonical domain state
```

**Step 6: Commit**

```bash
git commit -m "docs: update for simplified dual-store boundary (CF_CELLS only append-only)"
```

---

## Task 13: Final Verification

**Step 1: Full test suite**

```bash
cargo check && cargo clippy && cargo test
cd frontend && pnpm type-check && pnpm lint && pnpm test
```

**Step 2: Verify no remaining append-only references for moved CFs**

```bash
# Should find NO references to these CFs in append-only context:
rg "append_only.*CF_ACTIVITIES\|append_only.*CF_ADDR_TXS\|append_only.*CF_OBJECT_COLLECTION\|append_only.*CF_IDENTITY_COLLECTION" crates/
```

**Step 3: Commit any fixes**

```bash
git commit -m "chore: final cleanup for dual-store refactor"
```

---

## Post-Refactor Notes

**DB rebuild required:** Delete `data/domain/` and `data/append-only/`, re-sync from genesis.

**Performance re-evaluation:** After this refactor, the dual-store commit optimization picture changes:

- Append-only store now commits only CF_CELLS (T1 thread)
- Domain store commits everything else (T1b, T2, T4, T5, T6a, T6b, T7, T_ACT, finalize)
- The parallel dual-store commit optimization (commit domain + append-only simultaneously) now overlaps T1-cells-append with all other domain threads
- Writer double-buffering opportunity remains the same

**Re-evaluate optimizations** after this refactor lands and a fresh sync confirms correctness.
