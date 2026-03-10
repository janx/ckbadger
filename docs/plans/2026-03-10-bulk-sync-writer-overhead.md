# Bulk Sync Writer Overhead Elimination

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Eliminate ~1,787s of writer overhead (precompute + DB prefetch) per full sync, reducing wall time from ~3,246s to ~1,460s (~24 min).

**Architecture:** Two-part optimization: (A) Move writer precompute to parser stage where 65% idle capacity exists, sending owned precomputed data through the channel. (B) Add in-memory caches for addr_balance and script_info on the Indexer struct, replacing ~3M DB reads per sync with HashMap lookups. DAO/UDT prefetch remains but is negligible once addr/script reads are cached.

**Tech Stack:** Rust, RocksDB, tokio mpsc channels, rayon, std::thread::scope

---

## Background

Writer processes each batch in 3 phases:

1. **Precompute** (~250ms/batch): builds `all_cells`, `all_consumptions`, `cell_index_puts`, `cell_index_deletes`, `addr_tx_entries`, `changes_ref`, `batch_proposals` — pure computation on parsed data
2. **Prefetch** (~180ms/batch): 4-way `rayon::join` reads DAO deposits, UDT info, addr_balance, script_info from DB
3. **Write** (~300ms/batch): 9-thread `std::thread::scope` commits to RocksDB

Phases 1+2 total ~1,787s across 4,114 batches. Parser has ~1,998s idle capacity. Moving precompute to parser and caching addr/script makes the writer enter write phase immediately upon receiving a batch.

## Key Types Reference

- `ParsedBatch`: 22-element tuple type alias in `pipeline.rs:300-324`
- `CellIndexOp`: struct with 4 `Vec<u8>` / `Option<Vec<u8>>` fields (`batch.rs:3171-3176`)
- `AddressBalance`: 9-field struct (`types.rs:106-117`) — ~100 bytes per entry, ~3M unique addresses
- `ScriptInfo`: ~20-field struct (`types.rs:559-589`) — ~1K unique code_hashes
- `ScriptUsageChanges`: `HashMap<(Vec<u8>, bool), (i64, i64, i128, i128, i128, i128)>` (`batch.rs:637`)

---

### Task 1: Define CellIndexOp as a Shared Type

Currently `CellIndexOp` is a private struct inside `write_parsed_batch`. Move it to module scope so both parser and writer can use it.

**Files:**

- Modify: `crates/indexer/src/sync/batch.rs:3171-3176` (move struct definition)

**Step 1: Move CellIndexOp to module scope**

Move the `CellIndexOp` struct definition from inside `write_parsed_batch` (line 3171) to the top of the file near other type aliases (after `ScriptUsageChanges` at line 637). Add `pub(super)` visibility.

```rust
// Near line 637, after ScriptUsageChanges:
pub(super) struct CellIndexOp {
    pub lock_hash_key: Vec<u8>,
    pub lock_code_hash_key: Vec<u8>,
    pub type_hash_key: Option<Vec<u8>>,
    pub type_code_hash_key: Option<Vec<u8>>,
}
```

Remove the original `struct CellIndexOp` block at lines 3171-3176.

**Step 2: Verify compilation**

Run: `cargo check -p ckbadger-indexer`
Expected: compiles cleanly

**Step 3: Run tests**

Run: `cargo test -p ckbadger-indexer --lib`
Expected: all pass

**Step 4: Commit**

```bash
git add crates/indexer/src/sync/batch.rs
git commit -m "refactor: move CellIndexOp to module scope for cross-stage reuse"
```

---

### Task 2: Extract Precompute Into a Free Function

Extract the precompute block (lines 3100-3337) from `write_parsed_batch` into a standalone free function that can be called from either writer or parser.

**Files:**

- Modify: `crates/indexer/src/sync/batch.rs`

**Step 2a: Define the precompute return type**

Add near `CellIndexOp`:

```rust
pub(super) struct PrecomputedBatchData {
    pub all_cells: Vec<(Vec<u8>, i16, usize, i64)>,
    // (tx_hash, output_index, cell_index_in_tx_data_cells, block_number)
    // cell_index_in_tx_data_cells is used to index into all_tx_data[tx].cells[idx]
    pub all_consumptions: Vec<(Vec<u8>, i16, i64, Vec<u8>, i64, i16)>,
    // (prev_tx_hash, prev_output_index, created_at_block, consuming_tx_hash, consumed_at_block, input_index)
    pub cell_index_puts: Vec<CellIndexOp>,
    pub cell_index_deletes: Vec<CellIndexOp>,
    pub addr_tx_entries: Vec<(Vec<u8>, i64, i32, Vec<u8>)>,
    // (lock_hash, block_number, tx_index, tx_hash)
    pub changes_ref: HashMap<Vec<u8>, (i128, i32, i32, i64, i64, Vec<u8>, i128)>,
    // Owned version of address_balance_changes with Vec<u8> instead of &[u8]
}
```

**Step 2b: Extract the function**

Create a free function `precompute_batch_data` that takes the same inputs the precompute block currently reads and returns `Result<PrecomputedBatchData>`. The function body is the code currently at lines 3100-3293 (building `all_cells`, `all_consumptions`, `cell_index_puts`, `cell_index_deletes`, `addr_tx_entries`, `changes_ref`).

Key difference: `all_cells` and `all_consumptions` must use **owned** data (`Vec<u8>`) instead of borrowed slices (`&[u8]`), because they'll be sent across the channel. The function signature:

```rust
pub(super) fn precompute_batch_data(
    all_tx_data: &[TxData],
    input_cell_info: &HashMap<(Vec<u8>, i16), LiveCellInfo>,
    batch_cell_infos: &HashMap<(Vec<u8>, i16), LiveCellInfo>,
    address_balance_changes: &HashMap<Vec<u8>, (i128, i32, i32, i64, i64, Vec<u8>, i128)>,
) -> Result<PrecomputedBatchData>
```

Move the proposals code (lines 3297-3336) to remain inside `write_parsed_batch` — it uses `self` and should stay in the writer.

**Step 2c: Call the new function from write_parsed_batch**

Replace the precompute block in `write_parsed_batch` with:

```rust
let t_precompute = Instant::now();
let precomputed = precompute_batch_data(
    &all_tx_data,
    &input_cell_info,
    &batch_cell_infos,
    &address_balance_changes,
)?;
let PrecomputedBatchData {
    all_cells: all_cells_owned,
    all_consumptions: all_consumptions_owned,
    cell_index_puts,
    cell_index_deletes,
    addr_tx_entries,
    changes_ref,
} = precomputed;
```

Then build the reference vectors that the write threads need:

```rust
let all_cells: Vec<(&[u8], i16, &crate::parser::cell::ParsedCell, i64)> = all_cells_owned
    .iter()
    .map(|(tx_hash, output_index, cell_idx_in_tx, block_number)| {
        // Find the cell in all_tx_data
        // We need a helper to resolve cell_idx_in_tx back to the actual ParsedCell reference
        // This is a temporary step — in Task 4 we'll change the write threads to use owned data
    })
    .collect();
```

**Important**: This step is a _refactor only_ — behavior is identical. The writer still calls `precompute_batch_data` locally. We move it to the parser in Task 4.

**Step 2d: Verify compilation and tests**

Run: `cargo check -p ckbadger-indexer && cargo test -p ckbadger-indexer --lib`
Expected: compiles, all tests pass

**Step 2e: Commit**

```bash
git add crates/indexer/src/sync/batch.rs
git commit -m "refactor: extract precompute_batch_data free function from write_parsed_batch"
```

---

### Task 3: Change Write Threads to Use Owned Precomputed Data

The write threads currently use borrowed references (`&[u8]`) derived from `all_cells` and `all_consumptions`. Change them to accept the owned `PrecomputedBatchData` vectors directly, so the data can come from the channel.

**Files:**

- Modify: `crates/indexer/src/sync/batch.rs` (write phase: T1, T1b, T2 threads)

**Step 3a: Update all_cells to use owned data**

Currently `all_cells: Vec<(&[u8], i16, &ParsedCell, i64)>` — the write threads use `tx_hash` as `&[u8]` and `cell` as `&ParsedCell`.

Change `PrecomputedBatchData.all_cells` to store indices that reference `all_tx_data`:

```rust
// In PrecomputedBatchData:
pub all_cells_indices: Vec<(usize, usize, i64)>,
// (tx_data_index_in_all_tx_data, output_index_in_cells, block_number)
```

Then in the write phase, reconstruct the references from `all_tx_data`:

```rust
let all_cells: Vec<(&[u8], i16, &ParsedCell, i64)> = precomputed.all_cells_indices
    .iter()
    .map(|(tx_idx, cell_idx, block_num)| {
        let tx = &all_tx_data[*tx_idx];
        let cell = &tx.cells[*cell_idx];
        let output_index = checked_usize_to_i16(*cell_idx, "precomputed cell index").unwrap();
        (tx.hash.as_slice(), output_index, cell, *block_num)
    })
    .collect();
```

Similarly for `all_consumptions`.

**Step 3b: Verify compilation and tests**

Run: `cargo check -p ckbadger-indexer && cargo test -p ckbadger-indexer --lib`
Expected: compiles, all tests pass

**Step 3c: Commit**

```bash
git add crates/indexer/src/sync/batch.rs
git commit -m "refactor: write threads use PrecomputedBatchData indices into all_tx_data"
```

---

### Task 4: Extend ParsedBatch and Move Precompute to Parser

Add `PrecomputedBatchData` as a new field in `ParsedBatch`, compute it in the parser task, and remove the writer-side precompute call.

**Files:**

- Modify: `crates/indexer/src/sync/pipeline.rs:300-324` (ParsedBatch type)
- Modify: `crates/indexer/src/sync/pipeline.rs:~1760` (parser send)
- Modify: `crates/indexer/src/sync/pipeline.rs:~1834` (writer recv)
- Modify: `crates/indexer/src/sync/batch.rs` (write_parsed_batch signature)

**Step 4a: Add PrecomputedBatchData to ParsedBatch**

Extend the tuple type alias with one new field at the end:

```rust
type ParsedBatch = (
    u64, u64, u64, u64, u64,
    Arc<Vec<BlockResponseWithCycles>>,
    Vec<crate::parser::block::ParsedBlock>,
    Vec<TxData>,
    HashMap<(Vec<u8>, i16), LiveCellInfo>,
    HashMap<(Vec<u8>, i16), LiveCellInfo>,
    HashMap<Vec<u8>, (i128, i32, i32, i64, i64, Vec<u8>, i128)>,
    ScriptUsageChanges,
    HashMap<(Vec<u8>, bool, u32), (i128, i128)>,
    HashMap<(Vec<u8>, u32), (i128, i128)>,
    HashMap<Vec<u8>, SporeTypeIndex>,
    HashMap<(Vec<u8>, u32), (i128, i128)>,
    HashMap<(Vec<u8>, u32), (i128, i128)>,
    HashMap<Vec<u8>, ObjectTypeIndex>,
    HashMap<(Vec<u8>, u32), (i128, i128)>,
    PreParsedSporeData,
    PreParsedNftData,
    ParserBatchPerfSample,
    PrecomputedBatchData,  // NEW — precomputed in parser stage
);
```

**Step 4b: Compute in parser and send**

In the parser task, after building `all_tx_data`, `input_cell_info`, `batch_cell_infos`, `address_balance_changes`:

```rust
let precomputed = precompute_batch_data(
    &all_tx_data,
    &input_cell_info,
    &batch_cell_infos,
    &address_balance_changes,
)?;
```

Add `precomputed` as the last field in the `parse_tx.send((...))` call.

**Step 4c: Receive in writer and pass to write_parsed_batch**

Add `precomputed` to the destructuring pattern in the writer recv match arm. Pass it to `write_parsed_batch`.

**Step 4d: Update write_parsed_batch signature**

Add `precomputed: PrecomputedBatchData` parameter. Remove the `precompute_batch_data()` call inside the function — use the received `precomputed` directly.

**Step 4e: Verify compilation and tests**

Run: `cargo check -p ckbadger-indexer && cargo test -p ckbadger-indexer --lib`
Expected: compiles, all tests pass

**Step 4f: Commit**

```bash
git add crates/indexer/src/sync/batch.rs crates/indexer/src/sync/pipeline.rs
git commit -m "perf: move batch precompute from writer to parser stage

Overlaps precompute with previous batch's write phase.
Parser has ~65% idle capacity to absorb this work."
```

---

### Task 5: Add In-Memory addr_balance Cache to Indexer

Add a `HashMap<Vec<u8>, Option<AddressBalance>>` field to the `Indexer` struct. During bulk sync, use the cache instead of `read_address_balances()` DB calls.

**Files:**

- Modify: `crates/indexer/src/sync/indexer.rs:183-214` (Indexer struct)
- Modify: `crates/indexer/src/sync/indexer.rs:~222` (Indexer::new)
- Modify: `crates/indexer/src/sync/batch.rs` (prefetch section + post-commit cache update)
- Modify: `crates/indexer/src/sync/pipeline.rs` (pass cache ref to writer)

**Step 5a: Add field to Indexer struct**

```rust
// In Indexer struct, after latest_activities:
pub(crate) addr_balance_cache: std::sync::Mutex<HashMap<Vec<u8>, Option<AddressBalance>>>,
```

Initialize in `Indexer::new`:

```rust
addr_balance_cache: std::sync::Mutex::new(HashMap::new()),
```

**Step 5b: Replace DB read with cache lookup in writer**

In the prefetch section of `write_parsed_batch` (lines 3665-3680), replace the `writer.read_address_balances(&lock_hash_keys)` call with a cache lookup:

```rust
// Instead of: writer.read_address_balances(&lock_hash_keys)
// Use the cache passed from Indexer:
let mut cache_guard = addr_balance_cache.lock().unwrap();
let mut result = HashMap::with_capacity(lock_hash_keys.len());
for key in &lock_hash_keys {
    let cached = cache_guard.get(*key).cloned();
    // cached is Option<Option<AddressBalance>>:
    // - Some(Some(bal)): known address with balance
    // - Some(None): known address with no balance (seen before, never matched)
    // - None: never seen before → treat as None (new address)
    let value = cached.unwrap_or(None);
    result.insert((*key).clone(), value);
}
drop(cache_guard);
result
```

**Step 5c: Update cache after successful batch commit**

After the batch writes commit successfully (after `thread::scope` returns), update the cache with the new balances by applying the same deltas:

```rust
if bulk_sync_mode && !skip_address_balances && !changes_ref.is_empty() {
    let mut cache_guard = addr_balance_cache.lock().unwrap();
    for (lock_hash, (balance_delta, live_delta, total_delta, tx_delta, block_num, tx_hash, occupied_delta)) in &changes_ref {
        let entry = cache_guard.entry(lock_hash.clone()).or_insert(None);
        // Apply the same delta logic as apply_address_balance_deltas
        match entry {
            Some(bal) => {
                bal.balance += balance_delta;
                bal.occupied_capacity += occupied_delta;
                bal.live_cells_count += live_delta;
                bal.total_cells_count += *total_delta as i64;
                bal.txs_count += tx_delta;
                bal.last_activity_block = *block_num;
                bal.last_activity_tx = tx_hash.to_vec();
            }
            None => {
                *entry = Some(AddressBalance {
                    balance: *balance_delta,
                    occupied_capacity: *occupied_delta,
                    live_cells_count: *live_delta,
                    total_cells_count: *total_delta as i64,
                    txs_count: *tx_delta,
                    first_seen_block: *block_num,
                    first_seen_tx: tx_hash.to_vec(),
                    last_activity_block: *block_num,
                    last_activity_tx: tx_hash.to_vec(),
                });
            }
        }
    }
}
```

**Step 5d: Thread the cache reference through pipeline.rs**

The cache `&Mutex<HashMap<...>>` needs to reach `write_parsed_batch`. Since the writer loop has `&self` (the Indexer), pass `&self.addr_balance_cache` as an additional parameter.

**Step 5e: Clear cache when exiting bulk sync**

When transitioning from bulk sync to live sync, clear the cache to free memory:

```rust
// In the bulk→live transition path:
self.addr_balance_cache.lock().unwrap().clear();
self.addr_balance_cache.lock().unwrap().shrink_to_fit();
```

**Step 5f: Verify compilation and tests**

Run: `cargo check -p ckbadger-indexer && cargo test -p ckbadger-indexer --lib`
Expected: compiles, all tests pass

**Step 5g: Commit**

```bash
git add crates/indexer/src/sync/indexer.rs crates/indexer/src/sync/batch.rs crates/indexer/src/sync/pipeline.rs
git commit -m "perf: in-memory addr_balance cache eliminates ~3M DB reads during bulk sync

~300MB peak memory for ~3M addresses. Cache cleared on bulk→live transition."
```

---

### Task 6: Add In-Memory script_info Cache to Indexer

Same pattern as addr_balance but for script_info. Only ~1K entries, negligible memory.

**Files:**

- Modify: `crates/indexer/src/sync/indexer.rs` (Indexer struct + new)
- Modify: `crates/indexer/src/sync/batch.rs` (prefetch section + post-commit cache update)

**Step 6a: Add field to Indexer struct**

```rust
pub(crate) script_info_cache: std::sync::Mutex<HashMap<Vec<u8>, Option<ScriptInfo>>>,
```

Initialize: `script_info_cache: std::sync::Mutex::new(HashMap::new()),`

**Step 6b: Replace DB read with cache lookup**

In the prefetch section (lines 3682-3696), replace `writer.read_script_info(&code_hash_refs)` with cache lookup, same pattern as addr_balance.

**Step 6c: Update cache after commit**

Apply deltas from `script_usage_changes` to cached `ScriptInfo` entries after successful commit, using the same logic as `apply_script_usage_deltas`.

**Step 6d: Clear cache on bulk→live transition**

Same as addr_balance.

**Step 6e: Verify compilation and tests**

Run: `cargo check -p ckbadger-indexer && cargo test -p ckbadger-indexer --lib`
Expected: compiles, all tests pass

**Step 6f: Commit**

```bash
git add crates/indexer/src/sync/indexer.rs crates/indexer/src/sync/batch.rs
git commit -m "perf: in-memory script_info cache eliminates DB reads during bulk sync"
```

---

### Task 7: Simplify Bulk Sync Prefetch (DAO/UDT Only)

With addr_balance and script_info served from cache, the 4-way `rayon::join` can be simplified to 2-way (DAO + UDT only). This removes rayon overhead and simplifies the code.

**Files:**

- Modify: `crates/indexer/src/sync/batch.rs:3348-3717` (prefetch section)

**Step 7a: Remove addr_balance and script_info arms from rayon::join**

Change the 4-way `rayon::join(rayon::join(dao, udt), rayon::join(addr, script))` to `rayon::join(dao, udt)`. The addr_balance and script_info values are already resolved from cache in the code above.

**Step 7b: Verify compilation and tests**

Run: `cargo check -p ckbadger-indexer && cargo test -p ckbadger-indexer --lib`
Expected: compiles, all tests pass

**Step 7c: Commit**

```bash
git add crates/indexer/src/sync/batch.rs
git commit -m "refactor: simplify bulk prefetch to DAO+UDT only (addr/script served from cache)"
```

---

### Task 8: Add Perf Metrics for Cache Hits

Add timing and hit-rate metrics to verify the caches are working and measure the improvement.

**Files:**

- Modify: `crates/indexer/src/sync/batch.rs` (add cache timing in write_parsed_batch)
- Modify: `crates/indexer/src/sync/types.rs` (add cache_hit_rate field to BatchWriteMetrics)

**Step 8a: Add cache timing**

Wrap the cache lookup code with `Instant::now()` / `.elapsed()` and log at debug level:

```rust
debug!(
    addr_balance_cache_size = cache_guard.len(),
    script_info_cache_size = script_cache_guard.len(),
    cache_lookup_ms = cache_ms,
    "bulk sync cache stats"
);
```

**Step 8b: Verify and commit**

Run: `cargo check -p ckbadger-indexer && cargo test -p ckbadger-indexer --lib`

```bash
git add crates/indexer/src/sync/batch.rs crates/indexer/src/sync/types.rs
git commit -m "feat: add cache hit-rate metrics for bulk sync addr/script caches"
```

---

### Task 9: Integration Test — Full Pipeline Bulk Sync Path

Verify the full pipeline works end-to-end with the new precompute + cache path.

**Files:**

- Modify: existing test infrastructure or add integration test

**Step 9a: Run full test suite**

Run: `cargo test -p ckbadger-indexer`
Expected: all pass

**Step 9b: Run a short bulk sync (~1000 blocks) and verify**

Run: `cargo build -p ckbadger && ckbadger run` (after deleting DB)
Check logs for: precompute happening in parser, cache stats in writer, no panics.

**Step 9c: Run full bulk sync performance test**

Run full sync and compare with baseline (run-20260310T102854.429Z):

- wall_clock_seconds: baseline 3,246 → target ~1,460
- blocks_per_sec_wall: baseline 5,794 → target ~12,800
- avg_batch_seconds: baseline 0.738 → target ~0.355

**Step 9d: Commit any test fixes**

```bash
git add -A
git commit -m "test: verify bulk sync pipeline with precompute + cache optimizations"
```

---

## Execution Order

Tasks 1-4 are sequential (each builds on the previous).
Tasks 5-6 are independent of each other but depend on Tasks 1-4.
Task 7 depends on Tasks 5+6.
Task 8-9 depend on Task 7.

```
Task 1 → Task 2 → Task 3 → Task 4 ─┬─→ Task 5 ─┬─→ Task 7 → Task 8 → Task 9
                                      └─→ Task 6 ─┘
```

## Expected Results

| Metric                    | Before |   After |     Change |
| ------------------------- | -----: | ------: | ---------: |
| wall_clock_seconds        |  3,246 |  ~1,460 |       -55% |
| blocks_per_sec_wall       |  5,794 | ~12,800 |      +121% |
| avg_batch_seconds         |  0.738 |  ~0.355 |       -52% |
| precompute_ms (writer)    |   ~250 |      ~0 |      -100% |
| prefetch_ms (addr+script) |   ~180 |    ~0.1 |     -99.9% |
| Memory (peak addition)    |      0 |  ~300MB | addr cache |
