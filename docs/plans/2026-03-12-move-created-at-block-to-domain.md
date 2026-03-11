# Move `created_at_block` to Domain Store — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Remove position-dependent `created_at_block` from append-only `LiveCellInfo`, store it in domain (live marker value + `ConsumedCellMeta`) to fix infinite reorg retry loop.

**Architecture:** `created_at_block` moves from the append-only CF_CELLS value (LiveCellInfo) into two domain structures: the CF_LIVE_CELLS marker value (8 bytes LE for live cells) and `ConsumedCellMeta.created_at_block` (for consumed cells). All read paths reconstruct `created_at_block` from domain data. Append-only payloads become purely content-addressed.

**Tech Stack:** Rust, RocksDB, bincode serialization

**Migration:** Re-sync from genesis required. Delete `data/domain/` and `data/append-only/`, restart indexer.

**Design doc:** `docs/plans/2026-03-12-move-created-at-block-to-domain-design.md`

---

### Task 1: Update store types

**Files:**

- Modify: `crates/ckbadger-store/src/types.rs`

**Step 1: Remove `created_at_block` from `LiveCellInfo`**

Remove line 17 (`pub created_at_block: i64`). The struct becomes:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveCellInfo {
    pub capacity: i64,
    pub lock_script_hash: Vec<u8>,
    pub lock_code_hash: Vec<u8>,
    pub lock_hash_type: i16,
    pub lock_args: Vec<u8>,
    pub type_script_hash: Option<Vec<u8>>,
    pub type_code_hash: Option<Vec<u8>>,
    #[serde(default)]
    pub type_hash_type: Option<i16>,
    #[serde(default)]
    pub type_args: Option<Vec<u8>>,
    pub data_size: i32,
    #[serde(default)]
    pub occupied_capacity: i64,
    #[serde(default)]
    pub udt_amount: Option<u128>,
}
```

**Step 2: Add `created_at_block` to `ConsumedCellMeta`**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsumedCellMeta {
    pub consumed_at_block: i64,
    pub consumed_by_tx: Option<Vec<u8>>,
    pub created_at_block: i64,
}
```

**Step 3: Add `created_at_block` to `ConsumedCellInfo`**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsumedCellInfo {
    pub cell: LiveCellInfo,
    pub consumed_at_block: i64,
    pub consumed_by_tx: Option<Vec<u8>>,
    pub created_at_block: i64,
}
```

Update `from_live_cell_info_with_consumer` to take `created_at_block: i64` parameter:

```rust
impl ConsumedCellInfo {
    pub fn from_live_cell_info(info: &LiveCellInfo, consumed_at_block: i64, created_at_block: i64) -> Self {
        Self::from_live_cell_info_with_consumer(info, consumed_at_block, None, created_at_block)
    }

    pub fn from_live_cell_info_with_consumer(
        info: &LiveCellInfo,
        consumed_at_block: i64,
        consumed_by_tx: Option<&[u8]>,
        created_at_block: i64,
    ) -> Self {
        Self {
            cell: info.clone(),
            consumed_at_block,
            consumed_by_tx: consumed_by_tx.map(|tx| tx.to_vec()),
            created_at_block,
        }
    }

    pub fn to_live_cell_info(&self) -> LiveCellInfo {
        self.cell.clone()
    }
}
```

**Step 4: Add helper to decode `created_at_block` from live marker value**

```rust
/// Decode created_at_block from the live cell marker value (8 bytes LE).
pub fn decode_live_cell_marker(value: &[u8]) -> Option<i64> {
    if value.len() == 8 {
        Some(i64::from_le_bytes(value.try_into().unwrap()))
    } else {
        None
    }
}

/// Encode created_at_block for the live cell marker value.
pub fn encode_live_cell_marker(created_at_block: i64) -> [u8; 8] {
    created_at_block.to_le_bytes()
}
```

**Step 5: Do NOT compile yet** — downstream files will break. Continue to Task 2.

---

### Task 2: Update store batch write ops

**Files:**

- Modify: `crates/ckbadger-store/src/batch.rs`

**Step 1: Update `put_live_cell_marker` to write `created_at_block`**

Change signatures from:

```rust
pub fn put_live_cell_marker(&mut self, raw_key: &[u8]) {
    self.put_cf(self.store.cf_live_cells(), raw_key, []);
}
pub fn put_live_cell_marker_by_outpoint(&mut self, tx_hash: &[u8], output_index: i16) {
    let key = keys::encode_outpoint(tx_hash, output_index);
    self.put_live_cell_marker(&key);
}
```

To:

```rust
pub fn put_live_cell_marker(&mut self, raw_key: &[u8], created_at_block: i64) {
    self.put_cf(self.store.cf_live_cells(), raw_key, created_at_block.to_le_bytes());
}
pub fn put_live_cell_marker_by_outpoint(&mut self, tx_hash: &[u8], output_index: i16, created_at_block: i64) {
    let key = keys::encode_outpoint(tx_hash, output_index);
    self.put_live_cell_marker(&key, created_at_block);
}
```

**Step 2: Update `put_consumed_cell_meta_raw_key` to include `created_at_block`**

Change signature and body:

```rust
pub fn put_consumed_cell_meta_raw_key(
    &mut self,
    raw_key: &[u8],
    consumed_at_block: i64,
    consumed_by_tx: Option<&[u8]>,
    created_at_block: i64,
) {
    let consumed = ConsumedCellMeta {
        consumed_at_block,
        consumed_by_tx: consumed_by_tx.map(|tx| tx.to_vec()),
        created_at_block,
    };
    let value = bincode::serialize(&consumed).expect("serialize ConsumedCellMeta");
    self.put_cf(self.store.cf_consumed_cells(), raw_key, &value);
}
```

Update `put_consumed_cell_meta` similarly (add `created_at_block` param, pass through).

**Step 3: Update test-only unified helpers**

Update `put_cell` (line ~240) to take `created_at_block` separately — it currently reads from `info.created_at_block`. Change to accept it as a parameter:

```rust
pub fn put_cell_with_block(&mut self, tx_hash: &[u8], output_index: i16, info: &LiveCellInfo, created_at_block: i64) {
    let value = bincode::serialize(info).expect("serialize LiveCellInfo");
    let key = keys::encode_outpoint(tx_hash, output_index);
    self.put_cf(self.store.cf_cells(), &key, &value);
    self.put_cf(self.store.cf_live_cells(), &key, created_at_block.to_le_bytes());
}
```

Update `put_consumed_cell_with_consumer_raw_key` similarly to include `created_at_block` param.

**Step 4: Do NOT compile yet** — continue to Task 3.

---

### Task 3: Update store cell read ops

**Files:**

- Modify: `crates/ckbadger-store/src/cell_ops.rs`

The key change: read methods that return `LiveCellInfo` for live cells must also return `created_at_block` from the marker value.

**Step 1: Update `get_live_cell_by_outpoint_key`**

Return `Option<(LiveCellInfo, i64)>` — the i64 is `created_at_block`:

```rust
pub fn get_live_cell_by_outpoint_key(
    &self,
    outpoint_key: &[u8],
    cells_store: &CkbadgerStore,
) -> anyhow::Result<Option<(LiveCellInfo, i64)>> {
    let Some(marker_value) = self.get_cf(self.cf_live_cells(), outpoint_key)? else {
        return Ok(None);
    };
    let created_at_block = types::decode_live_cell_marker(&marker_value).ok_or_else(|| {
        anyhow::anyhow!(
            "invalid live cell marker value: outpoint=0x{}, value_len={}",
            bytes_to_hex(outpoint_key),
            marker_value.len()
        )
    })?;
    let info = cells_store.get_cell_by_outpoint_key(outpoint_key)?.ok_or_else(|| {
        anyhow::anyhow!(
            "missing canonical cell for live marker: outpoint=0x{}",
            bytes_to_hex(outpoint_key)
        )
    })?;
    Ok(Some((info, created_at_block)))
}
```

**Step 2: Update `get_cell`**

```rust
pub fn get_cell(
    &self,
    tx_hash: &[u8],
    output_index: i16,
    cells_store: &CkbadgerStore,
) -> anyhow::Result<Option<(LiveCellInfo, i64)>> {
    let key = keys::encode_outpoint(tx_hash, output_index);
    self.get_live_cell_by_outpoint_key(&key, cells_store)
}
```

**Step 3: Update `get_cells_batch`**

Change return type to `HashMap<(Vec<u8>, i16), (LiveCellInfo, i64)>`. Decode marker values instead of discarding:

In the loop over `live_values`, decode the marker value and save it alongside the position:

```rust
// Instead of just checking Ok(Some(_)), decode the marker value:
Ok(Some(marker_value)) => {
    let created_at_block = types::decode_live_cell_marker(&marker_value).ok_or_else(|| {
        anyhow::anyhow!(
            "invalid live cell marker value in get_cells_batch: outpoint=0x{}, value_len={}",
            bytes_to_hex(&keys[i]),
            marker_value.len()
        )
    })?;
    present_indices.push((i, created_at_block));
    // ...
}
```

Then when inserting results, include the `created_at_block`:

```rust
result.insert((tx_hash.to_vec(), idx), (info, created_at_block));
```

**Step 4: Update `get_consumed_cell_info`**

`ConsumedCellMeta` now has `created_at_block`. Pass it through to `ConsumedCellInfo`:

```rust
Ok(Some(ConsumedCellInfo {
    cell,
    consumed_at_block: meta.consumed_at_block,
    consumed_by_tx: meta.consumed_by_tx,
    created_at_block: meta.created_at_block,
}))
```

**Step 5: Update `get_consumed_cells_batch`**

This currently returns `HashMap<(Vec<u8>, i16), LiveCellInfo>`. Change return to `HashMap<(Vec<u8>, i16), (LiveCellInfo, i64)>` and extract `created_at_block` from the decoded consumed meta:

Save the decoded meta alongside present_indices, then when inserting results:

```rust
result.insert((tx_hash.to_vec(), idx), (info, created_at_block));
```

**Step 6: Update `list_cells_by_hash_cf` and `list_cells_by_code_hash_cf`**

These iterate cell index keys (which encode block_num at bytes 32..40) and then call `get_cell()`. Since `get_cell()` now returns `(LiveCellInfo, i64)`, update the return types:

```rust
-> anyhow::Result<Vec<(Vec<u8>, i16, LiveCellInfo, i64)>>
```

Or more practically, extract `created_at_block` from the index key directly (already available at `key[32..40]`):

```rust
let created_at_block = i64::from_be_bytes(key[32..40].try_into().unwrap());
```

**Step 7: Do NOT compile yet** — continue to Task 4.

---

### Task 4: Update store reorg ops

**Files:**

- Modify: `crates/ckbadger-store/src/reorg_ops.rs`

**Step 1: Update `delete_cell_index_entries` and `put_cell_index_entries`**

Add `created_at_block: i64` as a separate parameter instead of reading from `cell.created_at_block`:

```rust
fn delete_cell_index_entries(
    store: &CkbadgerStore,
    batch: &mut WriteBatch,
    cell: &LiveCellInfo,
    created_at_block: i64,
    tx_hash: &[u8],
    output_index: i16,
) {
    let idx_key = keys::encode_cell_index_key(&cell.lock_script_hash, created_at_block, tx_hash, output_index);
    // ... same pattern for all 4 index CFs, using created_at_block param
}
```

Same for `put_cell_index_entries`.

**Step 2: Update Fallback A (`delete_live_cells_after_tip_fallback`)**

Currently iterates `cf_live_cells()` and reads LiveCellInfo from cells_store to get `created_at_block`. Change to decode from marker value:

```rust
let iter = self.iterator_cf(self.cf_live_cells(), IteratorMode::Start);
for item in iter {
    let (key, marker_value) = item?;  // Now capture the value
    let created_at_block = types::decode_live_cell_marker(&marker_value).ok_or_else(|| {
        anyhow::anyhow!("invalid live cell marker during rollback: outpoint=0x{}", bytes_to_hex(&key))
    })?;
    if created_at_block > rollback_to {
        let (tx_hash, output_index) = keys::decode_outpoint(&key);
        let info = cells_store.get_cell_by_outpoint_key(&key)?...;
        batch.delete_cf(self.cf_live_cells(), &key);
        delete_cell_index_entries(self, &mut batch, &info, created_at_block, &tx_hash, output_index);
        // ...
    }
}
```

**Step 3: Update Fallback B (`restore_consumed_cells_fallback`)**

Read `created_at_block` from `ConsumedCellMeta` instead of `LiveCellInfo`:

```rust
let meta = decode_consumed_cell_meta(&value)?;
if meta.consumed_at_block <= rollback_to {
    continue;
}
let info = cells_store.get_cell_by_outpoint_key(&key)?...;
if meta.created_at_block <= rollback_to {
    batch.put_cf(self.cf_live_cells(), &key, meta.created_at_block.to_le_bytes());
    put_cell_index_entries(self, &mut batch, &info, meta.created_at_block, &tx_hash, output_index);
    // ...
}
```

**Step 4: Update TX-context path (`rollback_cells_from_tx_context`)**

For live cell deletion (outputs): decode `created_at_block` from marker value:

```rust
if let Some(marker_value) = self.get_cf(self.cf_live_cells(), &outpoint_key)? {
    let created_at_block = types::decode_live_cell_marker(&marker_value).ok_or_else(|| ...)?;
    let info = cells_store.get_cell_by_outpoint_key(&outpoint_key)?...;
    batch.delete_cf(self.cf_live_cells(), outpoint_key);
    delete_cell_index_entries(self, &mut batch, &info, created_at_block, &ctx.tx_hash, output_index);
    // ...
}
```

For consumed cell restoration (inputs): use `consumed.created_at_block`:

```rust
Some(consumed) => {
    // ...
    if consumed.created_at_block <= rollback_to {
        batch.put_cf(self.cf_live_cells(), outpoint_key, consumed.created_at_block.to_le_bytes());
        put_cell_index_entries(self, &mut batch, &consumed.cell, consumed.created_at_block, &input.tx_hash, input.output_index);
        // ...
    }
}
```

---

### Task 5: Update indexer writer/cells.rs

**Files:**

- Modify: `crates/indexer/src/db/writer/cells.rs`

**Step 1: Update `insert_cells_batch`**

Pass `created_at_block` from the cells tuple (element [3]) to the live marker:

```rust
// Line 143: was domain_batch.put_live_cell_marker(&raw_key);
domain_batch.put_live_cell_marker(&raw_key, *_created_at_block);
```

For cell index writes, use `*_created_at_block` instead of `info.created_at_block`.

**Step 2: Update `consume_cells_batch`**

When reading marker values, decode `created_at_block`:

```rust
// In the marker_results loop:
Ok(Some(marker_value)) => {
    let created_at_block = types::decode_live_cell_marker(&marker_value).ok_or_else(|| ...)?;
    present_positions.push((idx, created_at_block));
    // ...
}
```

In the consumption write loop, use `created_at_block` from the marker:

```rust
domain_batch.put_consumed_cell_meta_raw_key(
    &raw_key,
    *consumed_at_block,
    Some(*consumed_by_tx),
    marker_created_at_block,  // from marker value, not from info
);
// For index deletion:
domain_batch.delete_cell_by_lock(&info.lock_script_hash, marker_created_at_block, tx_hash, *output_index);
// ... same for other index CFs
```

**Step 3: Update `consume_cells_batch_preloaded`**

Here `created_at_block` comes from the consumption tuple element [2]:

```rust
domain_batch.put_consumed_cell_meta_raw_key(
    &raw_key,
    *consumed_at_block,
    Some(*consumed_by_tx),
    *_created_at_block,  // from tuple
);
// For index deletion, use *_created_at_block instead of info.created_at_block
```

**Step 4: Update `get_cells_info_batch` and `get_full_cells_info_batch`**

These return tuples/maps that include `created_at_block`. Update to get it from the domain source (the updated `get_cells_batch` / `get_consumed_cell_info` return values).

---

### Task 6: Update indexer pipeline

**Files:**

- Modify: `crates/indexer/src/sync/pipeline.rs`
- Modify: `crates/indexer/src/sync/batch.rs`
- Modify: `crates/indexer/src/sync/types.rs`

**Step 1: Update LiveCellInfo construction in pipeline.rs**

Remove `created_at_block` from all `LiveCellInfo { ... }` constructions:

- Line ~1041: Remove `created_at_block: tx_data.block_number,`
- Line ~758: Remove `created_at_block: cached.created_at_block,`

Keep `created_at_block` in `CachedCellInfo` (sync/types.rs:82) — the cache still needs to track it for the consumption tuple, just don't put it in LiveCellInfo.

**Step 2: Update consumption tuple construction in batch.rs**

Line ~1084: `info.created_at_block` — this field no longer exists on LiveCellInfo. The `created_at_block` should come from elsewhere. In the pipeline, when building the consumption tuple, we need the `created_at_block` of the cell being consumed.

For inputs that hit the cache: use `cached.created_at_block` from `CachedCellInfo`.
For inputs loaded from disk: use the `created_at_block` from `get_cells_info_batch` (which now returns it from domain).

The consumption tuple format `(tx_hash, output_index, created_at_block, consumed_by_tx, consumed_at_block, input_index)` remains — but `created_at_block` now comes from the cache or disk read, not from `info.created_at_block`.

**Step 3: Update CellIndexOp construction in batch.rs**

Lines ~1144-1171: These build cell index keys using `info.created_at_block`. Change to get `created_at_block` from the consumption tuple's element [2].

---

### Task 7: Update indexer other modules

**Files:**

- Modify: `crates/indexer/src/sync/reorg.rs` (~line 70)
- Modify: `crates/indexer/src/db/writer/hodl_wave.rs` (~line 126)

**Step 1: sync/reorg.rs**

Line 70: `tracker.cell_consumed(info.created_at_block, info.capacity)` — `info` is `LiveCellInfo` which no longer has `created_at_block`. Get it from the consumption context (the same `batch_cell_infos` or `input_cell_info` that provided `created_at_block` for the tuple).

**Step 2: hodl_wave.rs**

`cell_consumed(created_at_block, capacity)` — caller must provide `created_at_block` from domain. No change to the function signature itself.

---

### Task 8: Update API routes

**Files:**

- Modify: `crates/api/src/routes/cells.rs`
- Modify: `crates/api/src/routes/graph.rs`
- Modify: `crates/api/src/routes/statistics.rs`
- Modify: `crates/api/src/routes/scripts.rs`
- Modify: `crates/api/src/routes/spore.rs`
- Modify: `crates/api/src/routes/assets.rs`
- Modify: `crates/api/src/routes/identities.rs`
- Modify: `crates/api/src/warmup.rs`

**General pattern:** Where code reads `info.created_at_block` from a `LiveCellInfo`, destructure the new return tuple `(info, created_at_block)` from the cell read method.

Example in cells.rs:

```rust
// Before:
let info = store.get_cell(&tx_hash, output_index, cells_store)?.unwrap();
let block = info.created_at_block;

// After:
let (info, created_at_block) = store.get_cell(&tx_hash, output_index, cells_store)?.unwrap();
```

For consumed cells via `get_consumed_cell_info`:

```rust
// Before:
let consumed = store.get_consumed_cell_info(...)?.unwrap();
let created = consumed.cell.created_at_block;

// After:
let consumed = store.get_consumed_cell_info(...)?.unwrap();
let created = consumed.created_at_block;
```

**Note:** Spore/asset/identity routes that read `entry.created_at_block` from domain CFs (CF_SPORE_NFT_DOMAIN, etc.) are NOT affected — those are independent domain fields.

---

### Task 9: Fix all tests

**Files:**

- All test modules in modified files
- `crates/ckbadger-store/src/cell_ops.rs` tests
- `crates/ckbadger-store/src/batch.rs` tests
- `crates/indexer/src/db/writer/cells.rs` tests
- `crates/indexer/src/sync/batch.rs` tests
- `crates/indexer/src/sync/pipeline.rs` tests
- `crates/api/src/routes/*.rs` tests

**Step 1: Update `make_cell()` test helper in cell_ops.rs**

Remove `created_at_block: 100` from the `LiveCellInfo` construction.

**Step 2: Update `insert_cell()` test helper in cell_ops.rs**

Take `created_at_block` as a parameter, write it to the marker value:

```rust
store.put_cf(store.cf_live_cells(), &outpoint_key, &created_at_block.to_le_bytes()).unwrap();
```

**Step 3: Update all `LiveCellInfo { ... }` in tests**

Remove `created_at_block` field from every test construction.

**Step 4: Update assertion patterns**

Where tests assert on `info.created_at_block`, change to assert on the returned `created_at_block` from the tuple.

---

### Task 10: Add new targeted tests

**Step 1: Test live marker round-trip in cell_ops.rs tests**

```rust
#[test]
fn test_live_cell_marker_stores_created_at_block() {
    let (_dir, store) = test_store();
    let tx_hash = [0x11u8; 32];
    let cell = make_cell(100_00000000, 61_00000000, &[0x01; 32]);
    let created_at_block: i64 = 12345;
    // Insert with created_at_block in marker
    insert_cell(&store, &tx_hash, 0, &[0x01; 32], &cell, created_at_block);
    let (info, stored_block) = store.get_cell(&tx_hash, 0, &store).unwrap().unwrap();
    assert_eq!(stored_block, created_at_block);
    assert_eq!(info.capacity, cell.capacity);
}
```

**Step 2: Test append-only idempotency after reorg**

```rust
#[test]
fn test_append_only_cell_idempotent_after_reorg() {
    // Write cell payload to append-only
    // Write again with same key+value (simulating re-sync after reorg)
    // Should succeed (idempotent)
    // The key point: created_at_block is NOT in the payload, so values are identical
}
```

**Step 3: Test consumed cell preserves created_at_block**

```rust
#[test]
fn test_consumed_cell_meta_preserves_created_at_block() {
    // Create live cell with created_at_block=100
    // Consume it
    // Read consumed cell info
    // Assert created_at_block == 100
}
```

---

### Task 11: Compile and verify

**Step 1: Run cargo check**

```bash
cargo check
```

Fix any remaining compilation errors.

**Step 2: Run cargo clippy**

```bash
cargo clippy
```

**Step 3: Run all tests**

```bash
cargo test
```

**Step 4: Run frontend type-check**

```bash
cd frontend && pnpm type-check
```

(Frontend types shouldn't change — `createdAtBlock` in API responses is still populated.)

---

### Task 12: Commit

```bash
git add -A
git commit -m "fix: move created_at_block from append-only to domain store

Fixes infinite retry loop after reorg where append-only overwrite check
fails because created_at_block is position-dependent. Live cell markers
now store created_at_block (8 bytes LE) instead of empty value.
ConsumedCellMeta gains created_at_block field. LiveCellInfo no longer
contains created_at_block, making append-only payloads content-addressed.

Requires re-sync from genesis.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```
