# Identity Collection Activities Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Move DotBit and DID:CKB collection activities from the Object activity system to a dedicated Identity activity system, fixing the "missing collection aggregate" crash that blocks sync.

**Architecture:** Create parallel Identity collection CFs (`CF_IDENTITY_COLLECTION_ACTIVITIES` append-only, `CF_IDENTITY_AGG` domain) with `IdentityCollectionAggregate` type. Redirect DotBit and DID:CKB activity writes/reads to the new CFs. Reuse `ObjectCollectionActivityEntry` (its structure is standard-agnostic). The `ObjectCollectionActivityAccumulator` already accepts any collection_id so it can flush to either CF with a simple target parameter.

**Tech Stack:** Rust (RocksDB store, indexer pipeline), Axum API, existing key encoding

---

### Task 1: Add Identity Collection CFs and Types to Store

**Files:**

- Modify: `crates/ckbadger-store/src/store.rs:268-412` (CF constants, ALL_CFS, APPEND_CFS, accessor methods)
- Modify: `crates/ckbadger-store/src/types.rs:459-469` (add IdentityCollectionAggregate)

**Step 1: Add CF constants**

In `crates/ckbadger-store/src/store.rs`, after line 307 (CF_OBJECT_COLLECTION_ACTIVITIES), add:

```rust
pub const CF_IDENTITY_AGG: &str = "identity_agg";
pub const CF_IDENTITY_COLLECTION_ACTIVITIES: &str = "identity_collection_activities";
```

**Step 2: Add to ALL_CFS array**

In the `ALL_CFS` array (line 327-368), add the two new CFs before the closing `];`.

**Step 3: Add to APPEND_CFS**

In `APPEND_CFS` (line 412), add `CF_IDENTITY_COLLECTION_ACTIVITIES`:

```rust
pub const APPEND_CFS: &[&str] = &[CF_ADDR_TXS, CF_ACTIVITIES, CF_OBJECT_COLLECTION_ACTIVITIES, CF_IDENTITY_COLLECTION_ACTIVITIES];
```

**Step 4: Add HIGH_WRITE_CFS entry**

In `HIGH_WRITE_CFS` (line ~865-877), add `CF_IDENTITY_COLLECTION_ACTIVITIES`.

**Step 5: Add accessor methods**

After `cf_object_collection_activities()` (line 1164-1166), add:

```rust
pub fn cf_identity_agg(&self) -> &ColumnFamily {
    self.cf(CF_IDENTITY_AGG)
}

pub fn cf_identity_collection_activities(&self) -> &ColumnFamily {
    self.cf(CF_IDENTITY_COLLECTION_ACTIVITIES)
}
```

**Step 6: Add IdentityCollectionAggregate type**

In `crates/ckbadger-store/src/types.rs`, after `ObjectCollectionAggregate` (line ~469), add:

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IdentityCollectionAggregate {
    pub name: Option<String>,
    pub standard: IdentityStandard,
    pub total_count: i64,
    pub live_count: i64,
    pub holders_count: i64,
    pub activities_count: i64,
}
```

**Step 7: Verify compilation**

Run: `cargo check -p ckbadger-store`
Expected: PASS

**Step 8: Commit**

```bash
git add crates/ckbadger-store/src/store.rs crates/ckbadger-store/src/types.rs
git commit -m "feat(store): add CF_IDENTITY_AGG and CF_IDENTITY_COLLECTION_ACTIVITIES"
```

---

### Task 2: Add Identity Batch Write Methods

**Files:**

- Modify: `crates/ckbadger-store/src/batch.rs:741-803` (add identity variants)

**Step 1: Add put_identity_collection_aggregate**

After `put_object_collection_aggregate` (line ~748), add:

```rust
pub fn put_identity_collection_aggregate(
    &mut self,
    collection_id: &[u8],
    agg: &IdentityCollectionAggregate,
) {
    let value = bincode::serialize(agg).expect("serialize IdentityCollectionAggregate");
    self.put_cf(self.store.cf_identity_agg(), collection_id, &value);
}
```

**Step 2: Add put_identity_collection_activity**

After `put_object_collection_activity` (line ~801), add:

```rust
pub fn put_identity_collection_activity(
    &mut self,
    collection_id: &[u8],
    block_num: i64,
    tx_idx: i32,
    entry: &ObjectCollectionActivityEntry,
) {
    let key = keys::encode_nft_collection_activity_key(
        collection_id,
        block_num,
        tx_idx,
        &entry.block_hash,
        &entry.tx_hash,
    );
    let value = bincode::serialize(entry).expect("serialize identity collection activity");
    self.put_cf(
        self.store.cf_identity_collection_activities(),
        &key,
        &value,
    );
}
```

Note: Reuses `encode_nft_collection_activity_key` — the key format is standard-agnostic.

**Step 3: Verify compilation**

Run: `cargo check -p ckbadger-store`
Expected: PASS

**Step 4: Commit**

```bash
git add crates/ckbadger-store/src/batch.rs
git commit -m "feat(store): add identity collection batch write methods"
```

---

### Task 3: Add Identity Read Ops

**Files:**

- Modify: `crates/ckbadger-store/src/identity_ops.rs` (add aggregate + activity list methods)

**Step 1: Add get_identity_collection_aggregate**

In `identity_ops.rs`, add after existing methods:

```rust
pub fn get_identity_collection_aggregate(
    &self,
    collection_id: &[u8],
) -> anyhow::Result<Option<IdentityCollectionAggregate>> {
    match self.get_cf(self.cf_identity_agg(), collection_id)? {
        Some(value) => Ok(Some(bincode::deserialize(&value)?)),
        None => Ok(None),
    }
}
```

**Step 2: Add list_identity_collection_activities**

Mirror `list_object_collection_activities` from `object_ops.rs:366-408` but target `cf_identity_collection_activities()`:

```rust
pub fn list_identity_collection_activities(
    &self,
    collection_id: &[u8],
    limit: usize,
    cursor: Option<(i64, i32)>,
    action_filter: Option<&str>,
) -> anyhow::Result<Vec<(i64, i32, ObjectCollectionActivityEntry)>> {
    // Same logic as list_object_collection_activities but uses
    // self.cf_identity_collection_activities() instead of
    // self.cf_object_collection_activities()
}
```

Copy the implementation from `object_ops.rs:366-462` and change the CF accessor.

**Step 3: Add necessary imports to identity_ops.rs**

Add `use crate::types::{IdentityCollectionAggregate, ObjectCollectionActivityEntry};` and key-related imports.

**Step 4: Write unit tests**

Add tests analogous to the existing identity_ops tests, verifying round-trip for aggregate and activity list:

```rust
#[test]
fn test_identity_collection_aggregate_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let store = CkbadgerStore::open_domain(dir.path()).unwrap();
    let collection_id = *b"dotbit_collection_______________";

    let agg = IdentityCollectionAggregate {
        name: Some(".bit".to_string()),
        standard: IdentityStandard::DotBit,
        total_count: 100,
        live_count: 80,
        holders_count: 50,
        activities_count: 200,
    };

    let mut batch = StoreBatch::new(&store);
    batch.put_identity_collection_aggregate(&collection_id, &agg);
    batch.commit().unwrap();

    let loaded = store
        .get_identity_collection_aggregate(&collection_id)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.name, Some(".bit".to_string()));
    assert_eq!(loaded.activities_count, 200);
}
```

**Step 5: Run tests**

Run: `cargo test -p ckbadger-store -- identity`
Expected: PASS

**Step 6: Commit**

```bash
git add crates/ckbadger-store/src/identity_ops.rs
git commit -m "feat(store): add identity collection aggregate and activity read ops"
```

---

### Task 4: Add UndoSeqScope for Identity Collection Activities

**Files:**

- Modify: `crates/indexer/src/sync/types.rs:17-22`

**Step 1: Add new scope variant**

```rust
pub(crate) enum UndoSeqScope {
    TxContext = 0x0001,
    AppendAddrTx = 0x0002,
    AppendActivity = 0x0003,
    AppendObjectCollectionActivity = 0x0004,
    AppendIdentityCollectionActivity = 0x0005,
}
```

**Step 2: Verify compilation**

Run: `cargo check -p ckbadger-indexer`
Expected: PASS

**Step 3: Commit**

```bash
git add crates/indexer/src/sync/types.rs
git commit -m "feat(indexer): add UndoSeqScope::AppendIdentityCollectionActivity"
```

---

### Task 5: Add Identity Activity Accumulator Flush Variant

**Files:**

- Modify: `crates/indexer/src/db/writer/nft_activity_acc.rs`

The `ObjectCollectionActivityAccumulator::flush()` currently calls `batch.put_object_collection_activity()`. We need a variant that writes to the identity CF instead.

**Step 1: Add flush_identity method**

After `flush()` (line 84-149), add:

```rust
/// Same as `flush` but writes to the identity collection activities CF.
pub fn flush_identity(self, batch: &mut StoreBatch) -> Vec<(Vec<u8>, i64, i32, Vec<u8>, Vec<u8>)> {
    let mut inserted = Vec::new();
    for ((collection_id, tx_hash), entry) in self.entries {
        let mut per_object: HashMap<Vec<u8>, (bool, bool)> = HashMap::new();
        for (object_id, action) in &entry.object_actions {
            let pair = per_object
                .entry(object_id.clone())
                .or_insert((false, false));
            match action {
                RawAction::Create => pair.0 = true,
                RawAction::Consume => pair.1 = true,
            }
        }

        let mut actions = Vec::new();
        let mut has_mint = false;
        let mut has_transfer = false;
        let mut has_burn = false;
        for (created, consumed) in per_object.values() {
            match (*created, *consumed) {
                (true, true) => has_transfer = true,
                (true, false) => has_mint = true,
                (false, true) => has_burn = true,
                (false, false) => {}
            }
        }
        if has_mint {
            actions.push(AssetAction::Mint);
        }
        if has_transfer {
            actions.push(AssetAction::Transfer);
        }
        if has_burn {
            actions.push(AssetAction::Burn);
        }
        if actions.is_empty() {
            continue;
        }

        let activity_entry = ObjectCollectionActivityEntry {
            tx_hash: tx_hash.clone(),
            block_hash: entry.block_hash.clone(),
            timestamp_ms: entry.timestamp_ms,
            actions,
        };

        batch.put_identity_collection_activity(
            &collection_id,
            entry.block_number,
            entry.tx_idx,
            &activity_entry,
        );
        inserted.push((
            collection_id,
            entry.block_number,
            entry.tx_idx,
            entry.block_hash,
            tx_hash,
        ));
    }
    inserted
}
```

Note: The only difference from `flush()` is calling `put_identity_collection_activity` instead of `put_object_collection_activity`.

**Step 2: Add test for flush_identity**

Add a test similar to `test_mint_only` but using `flush_identity` and reading from `list_identity_collection_activities`.

**Step 3: Run tests**

Run: `cargo test -p ckbadger-indexer -- nft_activity_acc`
Expected: PASS

**Step 4: Commit**

```bash
git add crates/indexer/src/db/writer/nft_activity_acc.rs
git commit -m "feat(indexer): add flush_identity to ObjectCollectionActivityAccumulator"
```

---

### Task 6: Redirect DotBit Activity Writes to Identity CF

**Files:**

- Modify: `crates/indexer/src/db/writer/dotbit.rs:65-140`

**Step 1: Change resolve_dotbit_tx_activity to write to identity CF**

In `resolve_dotbit_tx_activity` (line 138), change:

```rust
// Before:
batch.put_object_collection_activity(&DOTBIT_SENTINEL_COLLECTION, block_number, tx_idx, &entry);
// After:
batch.put_identity_collection_activity(&DOTBIT_SENTINEL_COLLECTION, block_number, tx_idx, &entry);
```

**Step 2: Verify compilation**

Run: `cargo check -p ckbadger-indexer`
Expected: PASS

**Step 3: Commit**

```bash
git add crates/indexer/src/db/writer/dotbit.rs
git commit -m "fix(indexer): redirect dotbit activity writes to identity collection CF"
```

---

### Task 7: Redirect DID:CKB Activity Writes and Create Identity Delta Tracking in batch.rs

This is the largest task — updating the batch writer to separate identity and object activity deltas.

**Files:**

- Modify: `crates/indexer/src/sync/batch.rs`

Changes needed in **two** sync paths: bulk sync (~line 2536-2892) and grouped/live sync (~line 5384-5868).

**Step 1: Add apply_identity_collection_activity_count_deltas function**

After `apply_object_collection_activity_count_deltas_with_pending` (line 147-211), add:

```rust
fn apply_identity_collection_activity_count_deltas(
    store: &CkbadgerStore,
    batch: &mut StoreBatch,
    deltas: HashMap<Vec<u8>, i64>,
) -> Result<()> {
    if deltas.is_empty() {
        return Ok(());
    }

    for (collection_id, delta) in deltas {
        if delta == 0 {
            continue;
        }
        let mut agg = store
            .get_identity_collection_aggregate(&collection_id)?
            .unwrap_or_default();
        let next = agg.activities_count.checked_add(delta).ok_or_else(|| {
            anyhow!(
                "identity collection activities_count overflow: collection_id=0x{}, current={}, delta={}",
                hex::encode(&collection_id),
                agg.activities_count,
                delta
            )
        })?;
        if next < 0 {
            bail!(
                "identity collection activities_count underflow: collection_id=0x{}, current={}, delta={}",
                hex::encode(&collection_id),
                agg.activities_count,
                delta
            );
        }
        agg.activities_count = next;
        batch.put_identity_collection_aggregate(&collection_id, &agg);
    }
    Ok(())
}
```

Note: Unlike the Object variant, this uses `unwrap_or_default()` because Identity aggregates may not pre-exist (they bootstrap lazily). This is safe because `IdentityCollectionAggregate` derives `Default`.

**Step 2: Update bulk sync path (write_batch_sequential, ~line 2534-2892)**

In the bulk sync path:

a) Add a separate identity activity batch and delta tracker alongside the existing object ones:

```rust
let mut identity_activity_batch = StoreBatch::new(&self.append_only_store);
let mut identity_activity_count_deltas: HashMap<Vec<u8>, i64> = HashMap::new();
```

b) For DotBit activity writes (~line 2820-2857): change `object_activity_batch` to `identity_activity_batch`, `CF_OBJECT_COLLECTION_ACTIVITIES` to `CF_IDENTITY_COLLECTION_ACTIVITIES`, `UndoSeqScope::AppendObjectCollectionActivity` to `UndoSeqScope::AppendIdentityCollectionActivity`, and `object_activity_count_deltas` to `identity_activity_count_deltas`.

c) Add a separate identity accumulator for DID:CKB. Where DID:CKB records to `object_activity_acc` (~line 2588-2604), redirect to a new `identity_activity_acc` accumulator instead. Flush it with `flush_identity` into `identity_activity_batch`, and accumulate into `identity_activity_count_deltas`.

d) After the existing `apply_object_collection_activity_count_deltas_with_pending` call (~line 2887-2892), add:

```rust
apply_identity_collection_activity_count_deltas(
    self.writer.store(),
    &mut consume_batch,
    identity_activity_count_deltas,
)?;
```

e) After `object_activity_batch.commit()` (~line 2898), add:

```rust
if !identity_activity_batch.is_empty() {
    let commit_started = Instant::now();
    identity_activity_batch.commit()?;
    commit_ms += commit_started.elapsed().as_secs_f64() * 1000.0;
}
```

**Step 3: Update grouped/live sync path (~line 5384-5868)**

Apply the same pattern as Step 2 in the grouped sync path:

a) Add `identity_activity_batch` and `identity_activity_count_deltas`.

b) Create a separate `identity_activity_acc` alongside `object_activity_acc` (~line 5387).

c) Redirect DotBit activity writes (~line 5796-5832) to identity path.

d) Redirect DID:CKB `object_activity_acc.record()` calls (~lines 5448-5464) to `identity_activity_acc.record()`.

e) Flush `identity_activity_acc` with `flush_identity`, update undo entries with new scope and CF.

f) Call `apply_identity_collection_activity_count_deltas` after the object variant.

g) Commit identity_activity_batch.

**Step 4: Verify compilation**

Run: `cargo check -p ckbadger-indexer`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/indexer/src/sync/batch.rs
git commit -m "fix(indexer): separate identity activity deltas from object deltas in batch writer"
```

---

### Task 8: Update reorg_ops.rs for Identity Activities

**Files:**

- Modify: `crates/ckbadger-store/src/reorg_ops.rs:1650-1719`

**Step 1: Add identity activity counting in rollback repair**

After the existing object collection activity iteration (~line 1650-1705), add a parallel iteration over `cf_identity_collection_activities()`:

```rust
let mut identity_activity_totals: HashMap<Vec<u8>, i64> = HashMap::new();
let iter = activity_store.iterator_cf(
    activity_store.cf_identity_collection_activities(),
    IteratorMode::Start,
);
for item in iter {
    let (key, _) = item.map_err(|e| {
        anyhow::anyhow!(
            "failed to iterate identity_collection_activities while repairing rollback state: {}",
            e
        )
    })?;
    if key.len() != keys::NFT_COLLECTION_ACTIVITY_KEY_SIZE {
        continue;
    }
    let (collection_id, block_num, tx_idx, block_hash, tx_hash) =
        keys::decode_nft_collection_activity_key(&key);
    let Some((canonical_block_num, canonical_tx_idx)) = self.get_tx_location(&tx_hash)?
    else {
        continue;
    };
    if canonical_block_num != block_num || canonical_tx_idx != tx_idx {
        continue;
    }
    if self
        .get_tx_index(canonical_block_num, canonical_tx_idx)?
        .is_none()
    {
        continue;
    }
    let Some(canonical_header) = self.get_block_header(canonical_block_num)? else {
        anyhow::bail!(
            "missing block header while repairing rollback state from identity_collection_activities: block_num={}, tx_idx={}, tx_hash=0x{}",
            canonical_block_num,
            canonical_tx_idx,
            bytes_to_hex(&tx_hash)
        );
    };
    if canonical_header.hash != block_hash {
        continue;
    }
    let total = identity_activity_totals
        .entry(collection_id.to_vec())
        .or_insert(0);
    *total = total.checked_add(1).ok_or_else(|| {
        anyhow::anyhow!(
            "identity collection activities_count overflow while repairing rollback state"
        )
    })?;
}

// Write identity aggregates
for (collection_id, total) in &identity_activity_totals {
    let mut agg = self
        .get_identity_collection_aggregate(collection_id)?
        .unwrap_or_default();
    agg.activities_count = *total;
    let encoded = bincode::serialize(&agg).map_err(|e| {
        anyhow::anyhow!(
            "failed to serialize identity collection aggregate during rollback repair: collection_id=0x{}, error={}",
            bytes_to_hex(collection_id),
            e
        )
    })?;
    batch.put_cf(self.cf_identity_agg(), collection_id, &encoded);
}
```

**Step 2: Verify compilation**

Run: `cargo check -p ckbadger-store`
Expected: PASS

**Step 3: Commit**

```bash
git add crates/ckbadger-store/src/reorg_ops.rs
git commit -m "fix(store): handle identity collection activities in rollback repair"
```

---

### Task 9: Update API Read Paths

**Files:**

- Modify: `crates/api/src/routes/assets.rs:2401-2440` (collection activities endpoint)
- Modify: `crates/api/src/routes/assets.rs` (collection detail endpoint)
- Modify: `crates/api/src/utils/assets.rs:177-191` (resolve_collection_standard)

**Step 1: Update collection activities endpoint**

In `list_nft_collection_activities` (~line 2401), add a branch that checks if the collection_id is an identity sentinel. If so, call `store.list_identity_collection_activities()` instead of `store.list_object_collection_activities()`:

```rust
let is_identity_sentinel = collection_id_bytes == DOTBIT_SENTINEL_COLLECTION
    || collection_id_bytes == DID_CKB_SENTINEL_COLLECTION;

let activities = if is_identity_sentinel {
    store.list_identity_collection_activities(
        &collection_id_bytes,
        limit + 1,
        cursor,
        action_filter.as_deref(),
    )?
} else {
    store.list_object_collection_activities(
        &collection_id_bytes,
        limit + 1,
        cursor,
        action_filter.as_deref(),
    )?
};
```

**Step 2: Update collection detail endpoint**

Where the collection detail reads `ObjectCollectionAggregate` for sentinel IDs, change to read from `IdentityCollectionAggregate`:

```rust
let is_identity_sentinel = collection_id_bytes == DOTBIT_SENTINEL_COLLECTION
    || collection_id_bytes == DID_CKB_SENTINEL_COLLECTION;

if is_identity_sentinel {
    let agg = store
        .get_identity_collection_aggregate(&collection_id_bytes)?
        .ok_or_else(|| ApiError::not_found("Collection not found"))?;
    // Map IdentityCollectionAggregate fields to response
} else {
    let agg = store
        .get_object_collection_aggregate(&collection_id_bytes)?
        .ok_or_else(|| ApiError::not_found("Collection not found"))?;
    // Existing Object path
}
```

**Step 3: Verify compilation**

Run: `cargo check -p ckbadger-api`
Expected: PASS

**Step 4: Commit**

```bash
git add crates/api/src/routes/assets.rs crates/api/src/utils/assets.rs
git commit -m "fix(api): read identity collection activities from dedicated CF"
```

---

### Task 10: Update pipeline.rs Precompute Classification

**Files:**

- Modify: `crates/indexer/src/sync/pipeline.rs:1438-1448`

**Step 1: Update precompute classification**

The pipeline precompute currently classifies DotBit and DID:CKB sentinel collections into `object_type_index_cache` for prefetching. After this change, these sentinels should be tracked separately so the batch writer knows they're identity type.

Check if pipeline precompute needs an `identity_type_index_cache` or if the classification is only used for Object aggregate prefetch. If the sentinel classification is only used to pass collection_id downstream (not to prefetch ObjectCollectionAggregate), then no change may be needed here — the batch writer already routes by sentinel value.

Review `pipeline.rs:1438-1448` — if the sentinel collection_id is used only to feed `object_activity_acc.record()` in the batch writer, and we've already redirected that in Task 7, then pipeline.rs needs no changes.

If it IS used for aggregate prefetch, add an `identity_collections` set and skip prefetching `ObjectCollectionAggregate` for those.

**Step 2: Verify compilation**

Run: `cargo check -p ckbadger-indexer`
Expected: PASS

**Step 3: Commit if needed**

```bash
git add crates/indexer/src/sync/pipeline.rs
git commit -m "refactor(indexer): separate identity sentinel classification in pipeline precompute"
```

---

### Task 11: Update Integration Tests

**Files:**

- Modify: `crates/api/tests/api_integration.rs`

**Step 1: Update test fixtures**

Tests that create `ObjectCollectionAggregate` for dotbit/did_ckb sentinel IDs must switch to `IdentityCollectionAggregate` and use `put_identity_collection_aggregate`.

Key tests to update (search for `dotbit_collection` and `did_collection`):

- `test_assets_list_includes_did_ckb_collection_under_nft_type` (~line 4028)
- `test_assets_nft_collection_accepts_dotbit_alias` (~line 4398)
- `test_assets_nft_collection_accepts_did_ckb_aliases` (~line 4509)
- `test_assets_nft_list_uses_dotbit_display_name_when_aggregate_name_missing` (~line 4721)
- `test_assets_nft_collection_activities_supports_action_filter` (~line 5313)
- All tests that call `batch.put_object_collection_aggregate` with dotbit/did_ckb sentinel IDs

For activity tests, change `put_object_collection_activity` to `put_identity_collection_activity` for sentinel IDs.

**Step 2: Update batch.rs unit tests**

In `crates/indexer/src/sync/batch.rs`, test `apply_object_collection_activity_count_deltas_with_pending` (~line 6704-6748). Add a parallel test for `apply_identity_collection_activity_count_deltas` that verifies it bootstraps with `unwrap_or_default()`.

**Step 3: Run all tests**

Run: `cargo test`
Expected: PASS

Run: `cd frontend && npx vitest run`
Expected: PASS (no frontend changes expected)

**Step 4: Commit**

```bash
git add crates/api/tests/api_integration.rs crates/indexer/src/sync/batch.rs
git commit -m "test: update tests for identity collection activity migration"
```

---

### Task 12: Final Validation

**Step 1: Full pre-commit check**

Run: `cargo check && cargo clippy && cd frontend && pnpm type-check && pnpm lint`
Expected: PASS

**Step 2: Full test suite**

Run: `cargo test && cd frontend && npx vitest run`
Expected: PASS

**Step 3: Verify STORE_SCHEMA.md is up to date**

Update `docs/STORE_SCHEMA.md` to document the two new CFs:

- `CF_IDENTITY_AGG` (domain): Identity collection aggregate keyed by sentinel collection ID
- `CF_IDENTITY_COLLECTION_ACTIVITIES` (append-only): Identity collection activities keyed like object collection activities

**Step 4: Commit docs**

```bash
git add docs/STORE_SCHEMA.md
git commit -m "docs: add identity collection CFs to store schema"
```

---

## Validation Checklist

- [ ] `cargo check` passes
- [ ] `cargo clippy` passes — no warnings
- [ ] `cargo test` passes — all existing + new tests
- [ ] `cd frontend && npx vitest run` passes
- [ ] DotBit activities write to `CF_IDENTITY_COLLECTION_ACTIVITIES` not `CF_OBJECT_COLLECTION_ACTIVITIES`
- [ ] DID:CKB activities write to `CF_IDENTITY_COLLECTION_ACTIVITIES` not `CF_OBJECT_COLLECTION_ACTIVITIES`
- [ ] `apply_object_collection_activity_count_deltas_with_pending` no longer receives dotbit/did:ckb sentinel deltas
- [ ] Identity activity count deltas go through `apply_identity_collection_activity_count_deltas`
- [ ] API collection activities endpoint routes to correct CF based on sentinel check
- [ ] API collection detail endpoint reads from correct aggregate CF
- [ ] Rollback repair handles identity collection activities
- [ ] Re-sync required: YES (new CFs, delete RocksDB data directory and re-sync)

## Scope Summary

| Layer               | Files Changed                    | Reason                                                     |
| ------------------- | -------------------------------- | ---------------------------------------------------------- |
| Store types         | `types.rs`                       | Add `IdentityCollectionAggregate`                          |
| Store CFs           | `store.rs`                       | Add `CF_IDENTITY_AGG`, `CF_IDENTITY_COLLECTION_ACTIVITIES` |
| Store batch         | `batch.rs`                       | Add identity write methods                                 |
| Store read          | `identity_ops.rs`                | Add identity read methods                                  |
| Store reorg         | `reorg_ops.rs`                   | Handle identity CFs in rollback repair                     |
| Indexer types       | `sync/types.rs`                  | Add `UndoSeqScope::AppendIdentityCollectionActivity`       |
| Indexer accumulator | `nft_activity_acc.rs`            | Add `flush_identity`                                       |
| Indexer dotbit      | `dotbit.rs`                      | Write to identity CF                                       |
| Indexer batch       | `sync/batch.rs`                  | Separate identity/object delta tracking                    |
| Indexer pipeline    | `sync/pipeline.rs`               | Possibly update precompute classification                  |
| API routes          | `routes/assets.rs`               | Route identity sentinels to identity CFs                   |
| API utils           | `utils/assets.rs`                | Already correct (display-level only)                       |
| Tests               | `api_integration.rs`, `batch.rs` | Update fixtures and add identity tests                     |
| Docs                | `STORE_SCHEMA.md`                | Document new CFs                                           |
