# Write Dedup & Activity Builder Waste Elimination

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Eliminate redundant domain-store batch writes in mNFT/DotBit/Spore writers and wasted CPU in the activity builder.

**Architecture:** Defer batch writes to a single `flush_to_batch()` call per batch-state object (eliminating N-per-token writes), and make activity `CodeHashes` a process-lifetime static. All changes are behavior-preserving — final DB state is identical.

**Tech Stack:** Rust, RocksDB WriteBatch, `std::sync::OnceLock`

---

## Task 1: Make CodeHashes a process-lifetime static

**Files:**

- Modify: `crates/indexer/src/db/writer/activities.rs:23-104`

**Step 1: Run existing tests to establish baseline**

Run: `cargo test -p ckbadger-indexer -- activities --nocapture`
Expected: All PASS

**Step 2: Convert CodeHashes::new() to OnceLock static**

Replace the per-block `CodeHashes::new()` call at line 104 with a `static OnceLock`:

```rust
use std::sync::OnceLock;

static CODE_HASHES: OnceLock<CodeHashes> = OnceLock::new();

fn code_hashes() -> &'static CodeHashes {
    CODE_HASHES.get_or_init(CodeHashes::new)
}
```

Change `build_activities_for_block` (line 104) from:

```rust
let hashes = CodeHashes::new();
```

to:

```rust
let hashes = code_hashes();
```

Change `build_tx_activities` signature (line 153) from `hashes: &CodeHashes` — no change needed since the reference lifetime works.

**Step 3: Run tests to verify**

Run: `cargo test -p ckbadger-indexer -- activities --nocapture`
Expected: All PASS

**Step 4: Commit**

```
perf: make CodeHashes a process-lifetime static
```

---

## Task 2: Use cell_data directly in classify_output (eliminate hex re-decode)

**Files:**

- Modify: `crates/indexer/src/db/writer/activities.rs:377-426`

**Step 1: Fix UDT branch (lines 389-398)**

Replace:

```rust
if hashes.is_udt(type_code_hash) {
    if let Some(tsh) = type_script_hash {
        // Parse output data for UDT amount
        if let Some(data_hex) = outputs_data.get(output_idx) {
            let data = crate::rpc::parse_hex_to_bytes(data_hex);
            if let Some(amount) = UdtParser::parse_amount(&data) {
                let entry = accum.udt_deltas.entry(tsh.to_vec()).or_insert((0, 0));
                entry.1 += amount as i128;
            }
        }
    }
```

With:

```rust
if hashes.is_udt(type_code_hash) {
    if let Some(tsh) = type_script_hash {
        if let Some(amount) = UdtParser::parse_amount(cell_data) {
            let entry = accum.udt_deltas.entry(tsh.to_vec()).or_insert((0, 0));
            entry.1 += amount as i128;
        }
    }
```

**Step 2: Fix DAO branch (lines 400-426)**

Replace the DAO branch:

```rust
} else if type_code_hash == hashes.dao {
    // DAO output: deposit vs withdraw request
    if data_size == 8 {
        if let Some(data_hex) = outputs_data.get(output_idx) {
            let data_bytes = crate::rpc::parse_hex_to_bytes(data_hex);
            if data_bytes.len() != 8 {
                panic!(
                    "invalid DAO output data length while classifying activity: expected=8, got={}",
                    data_bytes.len()
                );
            }
            let bytes: [u8; 8] = data_bytes.as_slice().try_into().unwrap_or_else(|_| {
                panic!(
                    "failed to decode DAO output data while classifying activity: len={}",
                    data_bytes.len()
                )
            });
            let deposit_block = u64::from_le_bytes(bytes);
            if deposit_block == 0 {
                accum.dao_deposits.push(capacity);
            } else {
                accum
                    .dao_withdraw_requests
                    .push((capacity, deposit_block as i64));
            }
        }
    }
```

With:

```rust
} else if type_code_hash == hashes.dao {
    if cell_data.len() == 8 {
        let bytes: [u8; 8] = cell_data.try_into().unwrap_or_else(|_| {
            panic!(
                "DAO output data is not 8 bytes while classifying activity: len={}",
                cell_data.len()
            )
        });
        let deposit_block = u64::from_le_bytes(bytes);
        if deposit_block == 0 {
            accum.dao_deposits.push(capacity);
        } else {
            accum
                .dao_withdraw_requests
                .push((capacity, deposit_block as i64));
        }
    }
```

**Step 3: Remove unused parameters from classify_output**

Remove `output_idx` and `outputs_data` parameters since they're no longer used:

```rust
fn classify_output(
    accum: &mut OwnerAccum,
    type_code_hash: &[u8],
    type_script_hash: Option<&[u8]>,
    type_args: Option<&[u8]>,
    cell_data: &[u8],
    data_size: i32,
    hashes: &CodeHashes,
    capacity: i64,
) {
```

Update the call site (lines 199-211) to remove those arguments:

```rust
classify_output(
    accum,
    type_code_hash,
    cell.type_script_hash.as_deref(),
    cell.type_args.as_deref(),
    &cell.data,
    cell.data_size,
    hashes,
    cell.capacity,
);
```

NOTE: Also check whether `data_size` is still needed. After the change, the DAO branch uses `cell_data.len() == 8` instead of `data_size == 8`. Since `data_size` was derived from `cell_data` in ParsedCell, they are equivalent. Remove `data_size` from the signature too if it's no longer used anywhere in the function body.

**Step 4: Run tests**

Run: `cargo test -p ckbadger-indexer -- activities --nocapture`
Expected: All PASS

**Step 5: Commit**

```
perf: use cell_data directly in classify_output, remove hex re-decode
```

---

## Task 3: Eliminate per-owner peers clone

**Files:**

- Modify: `crates/indexer/src/db/writer/activities.rs:214-228`

**Step 1: Replace O(N^2) clone with collect-once + filter-clone**

The current code clones every peer hash for every owner. Instead, pre-collect references and only clone once when building the final peers vec:

Replace lines 214-228:

```rust
// Collect all lock hashes for peer computation
let all_lock_hashes: Vec<Vec<u8>> = owners.keys().cloned().collect();

let mut result = Vec::with_capacity(owners.len());

for (lock_hash, accum) in owners {
    let ckb_delta = accum.output_capacity - accum.input_capacity;
    let occupied_delta = accum.output_occupied - accum.input_occupied;

    // Peers = all other lock_hashes in this tx
    let peers: Vec<Vec<u8>> = all_lock_hashes
        .iter()
        .filter(|h| h.as_slice() != lock_hash.as_slice())
        .cloned()
        .collect();
```

With:

```rust
let mut result = Vec::with_capacity(owners.len());

for (lock_hash, accum) in &owners {
    let ckb_delta = accum.output_capacity - accum.input_capacity;
    let occupied_delta = accum.output_occupied - accum.input_occupied;

    // Peers = all other lock_hashes in this tx
    let peers: Vec<Vec<u8>> = owners
        .keys()
        .filter(|h| h.as_slice() != lock_hash.as_slice())
        .cloned()
        .collect();
```

This eliminates the `all_lock_hashes` intermediate vector and its N clones. We iterate `owners.keys()` directly (no allocation) and only clone the N-1 peers (same as before, but without the pre-clone of all N keys).

NOTE: Since `owners` is now borrowed by the loop (`for (lock_hash, accum) in &owners`), you'll need to update the rest of the loop body to use references rather than owned values. The `lock_hash` is now `&Vec<u8>` instead of `Vec<u8>`. Update line 322 from:

```rust
result.push((lock_hash, entry));
```

to:

```rust
result.push((lock_hash.clone(), entry));
```

**Step 2: Run tests**

Run: `cargo test -p ckbadger-indexer -- activities --nocapture`
Expected: All PASS

**Step 3: Commit**

```
perf: eliminate intermediate all_lock_hashes allocation in activity peers
```

---

## Task 4: Defer MnftBatchState batch writes

**Files:**

- Modify: `crates/indexer/src/db/writer/mnft.rs:15-140` (state), `334-560` (callers)
- Modify: `crates/indexer/src/sync/batch.rs` (add flush calls)

### Step 1: Add dirty tracking to MnftBatchState

Add dirty-key tracking fields and a `flush_to_batch` method:

```rust
#[derive(Default)]
pub(crate) struct MnftBatchState {
    tokens: HashMap<Vec<u8>, Option<NftEntry>>,
    collection_aggs: HashMap<Vec<u8>, Option<NftCollectionAggregate>>,
    dirty_collection_aggs: HashSet<Vec<u8>>,
    collection_owner_counts: HashMap<(Vec<u8>, Vec<u8>), i64>,
    dirty_owner_counts: HashSet<(Vec<u8>, Vec<u8>)>,
    hourly_transfers: HashMap<Vec<u8>, i64>,
    dirty_hourly_transfers: HashSet<Vec<u8>>,
}
```

Add `use std::collections::HashSet;` at top.

### Step 2: Change put_collection_aggregate to defer writes

Remove `batch` parameter, only update in-memory + mark dirty:

```rust
fn put_collection_aggregate(
    &mut self,
    collection_id: &[u8],
    agg: NftCollectionAggregate,
) {
    self.dirty_collection_aggs.insert(collection_id.to_vec());
    self.collection_aggs
        .insert(collection_id.to_vec(), Some(agg));
}
```

### Step 3: Change put_collection_owner_count to defer writes

```rust
fn put_collection_owner_count(
    &mut self,
    collection_id: &[u8],
    lock_hash: &[u8],
    count: i64,
) {
    let key = (collection_id.to_vec(), lock_hash.to_vec());
    self.dirty_owner_counts.insert(key.clone());
    self.collection_owner_counts.insert(key, count);
}
```

### Step 4: Change delete_collection_owner to defer writes

```rust
fn delete_collection_owner(
    &mut self,
    collection_id: &[u8],
    lock_hash: &[u8],
) {
    let key = (collection_id.to_vec(), lock_hash.to_vec());
    self.dirty_owner_counts.insert(key.clone());
    self.collection_owner_counts.insert(key, 0);
}
```

### Step 5: Defer hourly transfer batch writes

Currently the batch write is inline in the caller (insert_mnft_token_with_state, line 477). Move the `batch.put_nft_hourly_transfer(...)` call into the state and defer it:

Change `put_hourly_transfer` to mark dirty:

```rust
fn put_hourly_transfer(&mut self, key: Vec<u8>, count: i64) {
    self.dirty_hourly_transfers.insert(key.clone());
    self.hourly_transfers.insert(key, count);
}
```

Remove the inline `batch.put_nft_hourly_transfer(...)` at line 477 from `insert_mnft_token_with_state`. The `state.put_hourly_transfer(key, next)` call stays.

### Step 6: Add flush_to_batch method

```rust
pub(crate) fn flush_to_batch(&self, batch: &mut StoreBatch) {
    for id in &self.dirty_collection_aggs {
        if let Some(Some(agg)) = self.collection_aggs.get(id) {
            batch.put_nft_collection_aggregate(id, agg);
        }
    }
    for (cid, lh) in &self.dirty_owner_counts {
        let count = self
            .collection_owner_counts
            .get(&(cid.clone(), lh.clone()))
            .copied()
            .unwrap_or(0);
        if count > 0 {
            batch.put_nft_collection_owner_count(cid, lh, count);
        } else {
            batch.delete_nft_collection_owner(cid, lh);
        }
    }
    for key in &self.dirty_hourly_transfers {
        if let Some(&count) = self.hourly_transfers.get(key) {
            batch.put_stats(key, &count.to_le_bytes());
        }
    }
}
```

### Step 7: Update all callers

The following callers pass `batch` to the state methods — remove the `batch` argument:

In `apply_mnft_owner_transition`:

- Line 178: `state.delete_collection_owner(collection_id, old_lock, batch)` → `state.delete_collection_owner(collection_id, old_lock)`
- Line 181: `state.put_collection_owner_count(collection_id, old_lock, old_count - 1, batch)` → `state.put_collection_owner_count(collection_id, old_lock, old_count - 1)`
- Line 205: `state.put_collection_owner_count(collection_id, new_lock, next, batch)` → `state.put_collection_owner_count(collection_id, new_lock, next)`

In `insert_mnft_class_with_state`:

- Line 299: `state.put_collection_aggregate(&class.class_id, agg, batch)` → `state.put_collection_aggregate(&class.class_id, agg)`

In `insert_mnft_token_with_state`:

- Line 409: `state.put_collection_aggregate(&token.class_id, agg, batch)` → `state.put_collection_aggregate(&token.class_id, agg)`
- Line 436: same
- Line 462: same
- Line 477: REMOVE `batch.put_nft_hourly_transfer(...)` line (deferred to flush)

In `consume_mnft_token_with_state`:

- Line 550: `state.put_collection_aggregate(cid, agg, batch)` → `state.put_collection_aggregate(cid, agg)`

### Step 8: Add flush calls in batch.rs

Search for all locations where `mnft_state` is used and a batch is committed afterward. Add `mnft_state.flush_to_batch(&mut batch)` before each commit.

Key locations (search for `mnft_state` in batch.rs):

1. Pipeline mode insertion phase: before `nft_batch.commit()` (~line 2693)
2. Pipeline NFT activity phase: before commit (~line 4266 area)
3. Sequential sync mode: before the relevant commit (~line 5397 area)

Pattern:

```rust
mnft_state.flush_to_batch(&mut nft_batch);
nft_batch.commit()?;
```

### Step 9: Update tests

The existing unit tests in mnft.rs that call `insert_mnft_token_with_state` etc. won't need changes to the test LOGIC, but the non-`_with_state` convenience methods (`insert_mnft_token`, `insert_mnft_class`, `consume_mnft_token`) create their own short-lived state that is dropped without flush. These need updating:

For `insert_mnft_class` (line 250-259): flush state before returning.
For `insert_mnft_token` (line 313-332): flush state before returning.
For `consume_mnft_token` (line 485-494): flush state before returning.

Example fix for `insert_mnft_token`:

```rust
pub fn insert_mnft_token(...) -> Result<()> {
    let mut state = self.new_mnft_batch_state();
    self.insert_mnft_token_with_state(..., &mut state)?;
    state.flush_to_batch(batch);
    Ok(())
}
```

### Step 10: Run tests

Run: `cargo test -p ckbadger-indexer -- mnft --nocapture`
Expected: All PASS

### Step 11: Commit

```
perf: defer MnftBatchState batch writes to single flush
```

---

## Task 5: Defer DotbitBatchState batch writes

**Files:**

- Modify: `crates/indexer/src/db/writer/dotbit.rs:176-286` (state), `369-568` (callers)
- Modify: `crates/indexer/src/sync/batch.rs` (add flush calls)

Same pattern as Task 4 but for DotbitBatchState. Key differences:

- DotBit uses a SINGLE sentinel collection key, so `dirty_collection_agg` is just a `bool` flag
- `put_collection_aggregate` has no `collection_id` param (sentinel is implicit)

### Step 1: Add dirty tracking

```rust
#[derive(Default)]
pub(crate) struct DotbitBatchState {
    accounts: HashMap<Vec<u8>, Option<NftEntry>>,
    collection_agg_loaded: bool,
    collection_agg: Option<NftCollectionAggregate>,
    collection_agg_dirty: bool,
    collection_owner_counts: HashMap<Vec<u8>, i64>,
    dirty_owner_counts: HashSet<Vec<u8>>,
    hourly_transfers: HashMap<Vec<u8>, i64>,
    dirty_hourly_transfers: HashSet<Vec<u8>>,
}
```

### Step 2: Change put_collection_aggregate to defer

```rust
fn put_collection_aggregate(&mut self, agg: NftCollectionAggregate) {
    self.collection_agg = Some(agg);
    self.collection_agg_loaded = true;
    self.collection_agg_dirty = true;
}
```

### Step 3: Change put_collection_owner_count, delete_collection_owner to defer

Same pattern as Task 4 Steps 3-4 but with single-key (lock_hash only, no collection_id in the dirty set since it's always the sentinel).

```rust
fn put_collection_owner_count(&mut self, lock_hash: &[u8], count: i64) {
    self.dirty_owner_counts.insert(lock_hash.to_vec());
    self.collection_owner_counts.insert(lock_hash.to_vec(), count);
}

fn delete_collection_owner(&mut self, lock_hash: &[u8]) {
    self.dirty_owner_counts.insert(lock_hash.to_vec());
    self.collection_owner_counts.insert(lock_hash.to_vec(), 0);
}
```

### Step 4: Defer hourly transfer writes

Same as Task 4 Step 5 — mark dirty in `put_hourly_transfer`, remove inline `batch.put_nft_hourly_transfer(...)` from `insert_dotbit_account_with_state` (line 496).

### Step 5: Add flush_to_batch method

```rust
pub(crate) fn flush_to_batch(&self, batch: &mut StoreBatch) {
    if self.collection_agg_dirty {
        if let Some(agg) = &self.collection_agg {
            batch.put_nft_collection_aggregate(&DOTBIT_SENTINEL_COLLECTION, agg);
        }
    }
    for lh in &self.dirty_owner_counts {
        let count = self.collection_owner_counts.get(lh).copied().unwrap_or(0);
        if count > 0 {
            batch.put_nft_collection_owner_count(&DOTBIT_SENTINEL_COLLECTION, lh, count);
        } else {
            batch.delete_nft_collection_owner(&DOTBIT_SENTINEL_COLLECTION, lh);
        }
    }
    for key in &self.dirty_hourly_transfers {
        if let Some(&count) = self.hourly_transfers.get(key) {
            batch.put_stats(key, &count.to_le_bytes());
        }
    }
}
```

### Step 6: Update callers (same pattern as Task 4 Step 7)

Remove `batch` from all calls to state methods:

- `apply_dotbit_owner_transition`: `state.delete_collection_owner(old_lock, batch)` → `state.delete_collection_owner(old_lock)`, etc.
- `insert_dotbit_account_with_state`: `state.put_collection_aggregate(agg, batch)` → `state.put_collection_aggregate(agg)`, remove inline hourly batch write
- `consume_dotbit_account_with_state`: same pattern

Update convenience methods (`insert_dotbit_account`, `consume_dotbit_account`) to flush before returning.

### Step 7: Add flush calls in batch.rs

Same locations as Task 4 Step 8 but for `dotbit_state`.

For the consumption phase specifically: `dotbit_state.flush_to_batch(&mut consume_batch)` must happen BEFORE `dotbit_state.extend_pending_collection_aggregates(...)` to ensure the aggregate flush and activity-count update don't conflict. Actually, since `extend_pending_collection_aggregates` reads from in-memory (not batch), and `apply_nft_collection_activity_count_deltas_with_pending` writes to batch, the flush and activity-count-update will both write the aggregate key. The activity-count-update writes LAST and wins, which is correct (it includes the activity_count delta). This is acceptable: 2 writes per distinct collection (not per account).

### Step 8: Run tests

Run: `cargo test -p ckbadger-indexer -- dotbit --nocapture`
Expected: All PASS

### Step 9: Commit

```
perf: defer DotbitBatchState batch writes to single flush
```

---

## Task 6: Defer SporeBatchState batch writes

**Files:**

- Modify: `crates/indexer/src/db/writer/spore.rs:22-250` (state), `450-870` (callers)
- Modify: `crates/indexer/src/sync/batch.rs` (add flush calls)

Same pattern as Tasks 4-5 but for SporeBatchState, which has TWO aggregate types:

1. **Cluster aggregates**: `ClusterAggregate` keyed by `cluster_id` (same pattern as mNFT)
2. **did:ckb collection aggregate**: `NftCollectionAggregate` with sentinel key (same pattern as DotBit)

### Step 1: Add dirty tracking

```rust
#[derive(Default)]
pub(crate) struct SporeBatchState {
    spores: HashMap<Vec<u8>, Option<DobEntry>>,
    cluster_aggs: HashMap<Vec<u8>, ClusterAggregate>,
    dirty_cluster_aggs: HashSet<Vec<u8>>,
    cluster_owner_counts: HashMap<(Vec<u8>, Vec<u8>), i64>,
    dirty_cluster_owner_counts: HashSet<(Vec<u8>, Vec<u8>)>,
    spore_hourly_transfers: HashMap<Vec<u8>, i64>,
    dirty_spore_hourly_transfers: HashSet<Vec<u8>>,
    did_collection_agg_loaded: bool,
    did_collection_agg: Option<NftCollectionAggregate>,
    did_collection_agg_dirty: bool,
    did_owner_counts: HashMap<Vec<u8>, i64>,
    dirty_did_owner_counts: HashSet<Vec<u8>>,
    did_hourly_transfers: HashMap<Vec<u8>, i64>,
    dirty_did_hourly_transfers: HashSet<Vec<u8>>,
    spore_outpoints: HashMap<(Vec<u8>, i16), Vec<u8>>,
}
```

### Step 2: Defer all put/delete methods

Apply the same pattern from Tasks 4-5 to:

- `put_cluster_aggregate`: remove `batch`, mark dirty
- `put_cluster_owner_count`: remove `batch`, mark dirty
- `delete_cluster_owner`: remove `batch`, mark dirty
- `put_spore_hourly_transfer`: mark dirty (remove inline batch write from caller)
- `put_did_collection_aggregate`: remove `batch`, mark dirty
- `put_did_owner_count`: remove `batch`, mark dirty
- `delete_did_owner`: remove `batch`, mark dirty
- `put_did_hourly_transfer`: mark dirty (remove inline batch write from caller)

### Step 3: Add flush_to_batch method

```rust
pub(crate) fn flush_to_batch(&self, batch: &mut StoreBatch) {
    // Cluster aggregates
    for id in &self.dirty_cluster_aggs {
        if let Some(agg) = self.cluster_aggs.get(id) {
            batch.put_cluster_aggregate(id, agg);
        }
    }
    // Cluster owner counts
    for (cid, lh) in &self.dirty_cluster_owner_counts {
        let count = self
            .cluster_owner_counts
            .get(&(cid.clone(), lh.clone()))
            .copied()
            .unwrap_or(0);
        if count > 0 {
            batch.put_cluster_owner_count(cid, lh, count);
        } else {
            batch.delete_cluster_owner(cid, lh);
        }
    }
    // Spore hourly transfers
    for key in &self.dirty_spore_hourly_transfers {
        if let Some(&count) = self.spore_hourly_transfers.get(key) {
            batch.put_stats(key, &count.to_le_bytes());
        }
    }
    // did:ckb collection aggregate
    if self.did_collection_agg_dirty {
        if let Some(agg) = &self.did_collection_agg {
            batch.put_nft_collection_aggregate(&DID_CKB_SENTINEL_COLLECTION, agg);
        }
    }
    // did:ckb owner counts
    for lh in &self.dirty_did_owner_counts {
        let count = self.did_owner_counts.get(lh).copied().unwrap_or(0);
        if count > 0 {
            batch.put_nft_collection_owner_count(&DID_CKB_SENTINEL_COLLECTION, lh, count);
        } else {
            batch.delete_nft_collection_owner(&DID_CKB_SENTINEL_COLLECTION, lh);
        }
    }
    // did:ckb hourly transfers
    for key in &self.dirty_did_hourly_transfers {
        if let Some(&count) = self.did_hourly_transfers.get(key) {
            batch.put_stats(key, &count.to_le_bytes());
        }
    }
}
```

### Step 4: Update all callers

Same pattern as Tasks 4-5. Remove `batch` from state mutation calls in:

- `apply_owner_transition` (cluster)
- `apply_did_owner_transition`
- `insert_spore_cell_with_state` (cluster agg writes + did:ckb agg writes + hourly writes)
- `consume_spore` (cluster agg writes + did:ckb agg writes)
- `insert_cluster_cell_with_state` if it writes aggregates

### Step 5: Add flush calls in batch.rs

Same pattern. Add `spore_state.flush_to_batch(&mut batch)` before each commit where spore_state is used.

### Step 6: Run tests

Run: `cargo test -p ckbadger-indexer -- spore --nocapture`
Expected: All PASS

### Step 7: Commit

```
perf: defer SporeBatchState batch writes to single flush
```

---

## Task 7: Final verification

### Step 1: Full test suite

Run: `cargo test`
Expected: All PASS

### Step 2: Clippy check

Run: `cargo clippy`
Expected: No new warnings

### Step 3: Commit any final fixes

---

## Scope Summary

| File          | Change                                                      | Impact                                      |
| ------------- | ----------------------------------------------------------- | ------------------------------------------- |
| activities.rs | Static CodeHashes, direct cell_data, remove peers pre-clone | CPU: -10 parse_hex/block, -N^2 clones       |
| mnft.rs       | Deferred writes + flush                                     | Writes: N-per-collection → 1-per-collection |
| dotbit.rs     | Deferred writes + flush                                     | Writes: N-per-batch → 1 (sentinel key)      |
| spore.rs      | Deferred writes + flush                                     | Writes: N-per-cluster → 1-per-cluster       |
| batch.rs      | Add flush_to_batch calls before commits                     | Orchestration                               |

**Principle Alignment:**

- Local First: Reduces write amplification → faster sync
- Agent Friendly: No behavioral change, pure performance

**Re-sync required:** No
