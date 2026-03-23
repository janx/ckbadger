# Zero-Copy Activity Interface for History Materialization

## Problem

`build_history_rows_for_block` (the LEFT side of the reduce `rayon::join`) dominates the build phase at 157s / 20.8% of build_ms. Profiling reveals ~73% of that time is spent on heap allocations: ~1700M `.to_vec()` calls that copy `&[u8]` slices from the interner into owned `Vec<u8>` fields on intermediate structures (`ParsedCell`, `InputCellView`, `LiveCellInfo`), only to be consumed and dropped within the same batch.

### Root cause

Each output cell is reconstructed as an owned struct **twice**:
1. `parsed_cell_from_facts()` → `ParsedCell` (7-10 `.to_vec()`) for activity bundle input
2. `cell_facts_to_live_cell_info()` → `LiveCellInfo` (7-10 `.to_vec()`) for CF_CELLS serialization

Each input is reconstructed once:
3. `activity_input_view_from_resolved_input()` → `InputCellView` (7-10 `.to_vec()`)

Over 18.9M blocks / 96M cells / 95M inputs, this produces ~1700M small heap allocations (~55 GB throughput).

### Allocation budget (current)

| Source | Calls | Allocs | % |
|--------|------:|-------:|--:|
| `parsed_cell_from_facts` (activity) | 96M | 576M | 34% |
| `cell_facts_to_live_cell_info` (CF_CELLS) | 96M | 576M | 34% |
| `activity_input_view_from_resolved_input` | 95M | 473M | 28% |
| `parsed_udt_cell_from_*` (UDT detect) | ~10M | 76M | 4% |
| **Total** | | **1701M** | |

## Design

Replace owned `Vec<u8>` fields with borrowed `&'a [u8]` references in the activity detection interface. In the bulk sync path, these borrow directly from the interner (zero-copy). In the live sync path, they borrow from existing owned `LiveCellInfo` data. The only remaining `.to_vec()` calls are in the final `OwnerActivityDelta` construction — once per unique owner per tx (~125M, down from 1700M).

### Scope

This change touches the activity **interface types**, their **construction sites**, and internal functions that store into `OwnerAccum`. It does NOT change:
- `LiveCellInfo` struct or its bincode serialization format
- `TxActivityBundle` / `OwnerActivityDelta` struct (DB storage types)
- `cell_facts_to_live_cell_info` function (still needed for CF_CELLS rows)
- `build_history_rows_for_block` overall structure
- Any DB schema or wire format

**Lifetime propagation scope:** Adding `'a` to `OwnerAccum` ripples into internal helper functions that store borrowed data into it. These include `classify_input`, `classify_output`, `record_owner_lock_script`, `record_script_call`, `emit_object_changes`, and `emit_identity_changes`. Their parameter types already accept `&[u8]`, but the lifetime must be named `'a` so the borrow can flow into `OwnerAccum<'a>`.

**Deferred: `ParsedUdtCell` borrowing.** `ParsedUdtCell` accounts for only 4% of allocations. Making it borrowed requires changes to `UdtParser::build_transfers_from_cells` in `parser/udt.rs` (which uses `.clone()` on its fields for `BTreeMap` keys). This is deferred to keep scope focused on the 96% win.

### Changed types

#### 1. `InputCellView` → `InputCellView<'a>` (activities.rs)

```rust
// BEFORE
pub struct InputCellView {
    pub lock_script_hash: Vec<u8>,
    pub lock_code_hash: Vec<u8>,
    pub lock_hash_type: i16,
    pub lock_args: Vec<u8>,
    pub capacity: i64,
    pub occupied_capacity: i64,
    pub type_code_hash: Option<Vec<u8>>,
    pub type_hash_type: Option<i16>,
    pub type_script_hash: Option<Vec<u8>>,
    pub type_args: Option<Vec<u8>>,
    pub udt_amount: Option<u128>,
    pub data: Vec<u8>,
    pub is_dao_withdraw_request: bool,
    pub dao_compensation: Option<i64>,
}

// AFTER
pub struct InputCellView<'a> {
    pub lock_script_hash: &'a [u8],
    pub lock_code_hash: &'a [u8],
    pub lock_hash_type: i16,
    pub lock_args: &'a [u8],
    pub capacity: i64,
    pub occupied_capacity: i64,
    pub type_code_hash: Option<&'a [u8]>,
    pub type_hash_type: Option<i16>,
    pub type_script_hash: Option<&'a [u8]>,
    pub type_args: Option<&'a [u8]>,
    pub udt_amount: Option<u128>,
    pub data: &'a [u8],
    pub is_dao_withdraw_request: bool,
    pub dao_compensation: Option<i64>,
}
```

#### 2. New `OutputCellView<'a>` (activities.rs)

Replaces `&[ParsedCell]` in `TxView`. Contains the same fields as `ParsedCell` but borrowed.

```rust
pub struct OutputCellView<'a> {
    pub capacity: i64,
    pub lock_code_hash: &'a [u8],
    pub lock_hash_type: i16,
    pub lock_args: &'a [u8],
    pub lock_script_hash: &'a [u8],
    pub type_code_hash: Option<&'a [u8]>,
    pub type_hash_type: Option<i16>,
    pub type_args: Option<&'a [u8]>,
    pub type_script_hash: Option<&'a [u8]>,
    pub data_hash: &'a [u8],
    pub data_size: i32,
    pub data: &'a [u8],
}
```

#### 3. `TxView<'a>` updated (activities.rs)

```rust
// BEFORE
pub struct TxView<'a> {
    pub tx_hash: &'a [u8],
    pub block_hash: &'a [u8],
    pub tx_index: i32,
    pub block_number: i64,
    pub timestamp: i64,
    pub is_cellbase: bool,
    pub inputs: Vec<InputCellView>,        // owned
    pub outputs: &'a [ParsedCell],         // borrows ParsedCell which owns Vec<u8>
}

// AFTER
pub struct TxView<'a> {
    pub tx_hash: &'a [u8],
    pub block_hash: &'a [u8],
    pub tx_index: i32,
    pub block_number: i64,
    pub timestamp: i64,
    pub is_cellbase: bool,
    pub inputs: Vec<InputCellView<'a>>,    // borrowed fields
    pub outputs: Vec<OutputCellView<'a>>,  // borrowed fields
}
```

#### 4. `OwnerAccum` → `OwnerAccum<'a>` (activities.rs)

Fields that stored owned bytes become borrowed. The HashMap key for owners also becomes `&'a [u8]`.

```rust
pub(crate) struct OwnerAccum<'a> {
    pub(crate) lock_code_hash: Option<&'a [u8]>,       // was Option<Vec<u8>>
    pub(crate) lock_hash_type: Option<i16>,
    pub(crate) lock_args: Option<&'a [u8]>,             // was Option<Vec<u8>>
    pub(crate) input_capacity: i128,
    pub(crate) output_capacity: i128,
    pub(crate) input_used: i64,
    pub(crate) output_used: i64,
    pub(crate) udt_deltas: HashMap<&'a [u8], (i128, i128)>,  // key was Vec<u8>
    pub(crate) dao_deposits: Vec<i64>,
    pub(crate) dao_withdraw_requests: Vec<(i64, i64)>,
    pub(crate) dao_withdraw_completes: Vec<(i64, i64)>,
    pub(crate) spore_inputs: Vec<&'a [u8]>,             // was Vec<Vec<u8>>
    pub(crate) spore_outputs: Vec<&'a [u8]>,
    pub(crate) nft_inputs: Vec<&'a [u8]>,
    pub(crate) nft_outputs: Vec<&'a [u8]>,
    pub(crate) dotbit_inputs: Vec<Vec<u8>>,              // STAYS owned (computed, not borrowed)
    pub(crate) dotbit_outputs: Vec<Vec<u8>>,
    pub(crate) did_ckb_inputs: Vec<&'a [u8]>,
    pub(crate) did_ckb_outputs: Vec<&'a [u8]>,
    pub(crate) involved_scripts: BTreeSet<&'a [u8]>,    // was BTreeSet<Vec<u8>>
    pub(crate) has_type_script: bool,
    pub(crate) unrecognized_type_calls: BTreeSet<(&'a [u8], i16, &'a [u8])>,  // was (Vec, i16, Vec)
    pub(crate) unrecognized_lock_calls: BTreeSet<(&'a [u8], i16, &'a [u8])>,
}
```

Note: `dotbit_inputs/outputs` stay `Vec<Vec<u8>>` because `resolve_dotbit_account_id` computes the account ID (not a direct borrow from facts).

#### 5. `build_tx_activity_bundle` changes (activities.rs)

The owners HashMap becomes `HashMap<&'a [u8], OwnerAccum<'a>>`. Entry API works with `&[u8]` keys.

```rust
fn build_tx_activity_bundle<'a, S: BuildHasher>(
    tx: &TxView<'a>,
    hashes: &CodeHashes,
    token_info_cache: &HashMap<Vec<u8>, (Option<String>, Option<u8>), S>,
    detectors: &[Box<dyn ProtocolDetector>],
) -> Result<TxActivityBundle> {
    let mut owners: HashMap<&'a [u8], OwnerAccum<'a>> = HashMap::new();

    for input in &tx.inputs {
        if input.lock_script_hash.len() < 32 { continue; }
        let accum = owners.entry(input.lock_script_hash).or_default();
        // ...
    }
    // ...
}
```

The final conversion to `OwnerActivityDelta` calls `.to_vec()` once per owner:

```rust
OwnerActivityDelta {
    lock_hash: lock_hash.to_vec(),                           // 1 alloc
    lock_code_hash: accum.lock_code_hash.unwrap().to_vec(),  // 1 alloc
    lock_args: accum.lock_args.unwrap().to_vec(),             // 1 alloc
    involved_script_code_hashes: accum.involved_scripts
        .iter().map(|s| s.to_vec()).collect(),                // N allocs (small N)
    // ...
}
```

#### 6. `ProtocolDetector` trait (activities.rs)

The trait signature stays the same (`fn detect(&self, tx: &TxView<'_>, ...)`) but the data behind `TxView` is now borrowed. Detectors that access `tx.outputs[i].type_code_hash` now get `Option<&[u8]>` instead of `&Option<Vec<u8>>`. Pattern matching and comparisons on `&[u8]` work identically.

### Construction sites

#### Bulk sync (mod.rs — `build_history_rows_for_block`)

**Before:** `parsed_cell_from_facts(cell, interner)` constructs `ParsedCell` with `.to_vec()`.

**After:** Construct `OutputCellView` directly from facts + interner (zero-copy):

```rust
let outputs: Vec<OutputCellView<'_>> = block_resolved.iter()
    .flat_map(|tx| tx.cells.iter())
    .map(|cell| OutputCellView {
        capacity: cell.capacity,
        lock_script_hash: interner.resolve_bytes(cell.lock_script_hash_id),
        lock_code_hash: interner.resolve_bytes(cell.lock_code_hash_id),
        lock_hash_type: cell.lock_hash_type,
        lock_args: interner.resolve_bytes(cell.lock_args_id),
        type_code_hash: cell.type_code_hash_id.map(|id| interner.resolve_bytes(id)),
        // ... all zero-copy
    })
    .collect();
```

Similarly, `InputCellView` is constructed from `ResolvedInputFacts` + interner without `.to_vec()`.

**`parsed_cell_from_facts` and `activity_input_view_from_resolved_input` are deleted.** Their only consumer was activity bundle construction.

**`cell_facts_to_live_cell_info` stays** — it still needs owned `Vec<u8>` for `LiveCellInfo` bincode serialization (CF_CELLS).

#### Live sync (batch.rs — `build_activity_input_views`)

**Before:** Clones `Vec<u8>` from `PositionedCellInfo` (which contains `LiveCellInfo`).

**After:** Borrows from existing `LiveCellInfo`:

```rust
Ok(InputCellView {
    lock_script_hash: &info.lock_script_hash,  // was .clone()
    lock_code_hash: &info.lock_code_hash,      // was .clone()
    lock_args: &info.lock_args,                // was .clone()
    type_code_hash: info.type_code_hash.as_deref(),  // was .clone()
    // ...
})
```

Similarly, outputs construction borrows from `ParsedCell`:

```rust
let outputs: Vec<OutputCellView<'_>> = td.cells.iter().map(|cell| OutputCellView {
    capacity: cell.capacity,
    lock_script_hash: &cell.lock_script_hash,
    lock_code_hash: &cell.lock_code_hash,
    // ...
}).collect();
```

### UDT cell parsing (deferred)

`parsed_udt_cell_from_output` / `parsed_udt_cell_from_input` construct `ParsedUdtCell` with `.to_vec()` (4% of total allocations). Making `ParsedUdtCell` borrowed would require changes to `UdtParser::build_transfers_from_cells` in `parser/udt.rs`, which uses `.clone()` on fields for `BTreeMap` keys. This is **deferred** — the functions continue to construct owned `ParsedUdtCell` from the (now borrowed) `OutputCellView`/`InputCellView` fields, calling `.to_vec()` as before. The 4% allocation cost is acceptable for the first pass.

### `record_owner_lock_script` changes

Currently validates that `lock_code_hash`, `lock_hash_type`, `lock_args` are consistent across cells for the same owner. With borrowed fields:

```rust
fn record_owner_lock_script<'a>(
    accum: &mut OwnerAccum<'a>,
    lock_code_hash: &'a [u8],
    lock_hash_type: i16,
    lock_args: &'a [u8],
) -> Result<()> {
    match (accum.lock_code_hash, accum.lock_hash_type, accum.lock_args) {
        (Some(existing), Some(ht), Some(args)) => {
            if existing != lock_code_hash { bail!(...) }
            if ht != lock_hash_type { bail!(...) }
            if args != lock_args { bail!(...) }
        }
        (None, None, None) => {
            accum.lock_code_hash = Some(lock_code_hash);  // store &'a [u8], no alloc
            accum.lock_hash_type = Some(lock_hash_type);
            accum.lock_args = Some(lock_args);
        }
        _ => bail!(...)
    }
    Ok(())
}
```

### Allocation budget (after)

| Source | Before | After | Change |
|--------|-------:|------:|-------:|
| `parsed_cell_from_facts` (activity) | 576M | 0 | eliminated |
| `cell_facts_to_live_cell_info` (CF_CELLS) | 576M | 576M | unchanged |
| `activity_input_view` | 473M | 0 | eliminated |
| `parsed_udt_cell` | 76M | 76M | deferred |
| `OwnerActivityDelta` final conversion | 0 | ~125M | new (was part of above) |
| **Total** | **1701M** | **~777M** | **-54%** |

The 576M remaining is `cell_facts_to_live_cell_info` for CF_CELLS — unchanged, not part of this work.

Net activity-related allocation reduction: **1125M → ~201M (82% reduction)**.

### Internal helper lifetime propagation

Functions inside `activities.rs` that store data into `OwnerAccum<'a>` need lifetime-annotated parameters so borrows can flow through:

- `classify_input`: `type_code_hash: &'a [u8]`, `type_script_hash: Option<&'a [u8]>`, `type_args: Option<&'a [u8]>`, `data: &'a [u8]` — so that `accum.spore_inputs.push(args)` (was `.to_vec()`) stores the borrow.
- `classify_output`: same pattern.
- `record_script_call` (called from classify_*): parameters carry `'a` so `accum.unrecognized_type_calls.insert((type_code_hash, hash_type, args))` stores borrows.
- `emit_object_changes` / `emit_identity_changes`: signature changes from `inputs: &[Vec<u8>]` to `inputs: &[&'a [u8]]` (or generic over `AsRef<[u8]>`). These functions produce `AssetChange` variants with owned `Vec<u8>` fields, so they call `.to_vec()` at the boundary — same allocation count as current, just moved to the output site.

### `data` field sourcing

In the bulk sync path, `OutputCellView.data` borrows from `CellFacts.data: Vec<u8>` (not from the interner). This is sound because `arena_cells` outlives the per-block processing. In the live sync path, `InputCellView.data` is `&[]` (static empty slice for inputs where data is unavailable).

## Testing

Test helper functions in 5 files (`activities.rs`, `fiber_detector.rs`, `stablepp_detector.rs`, `utxoswap_detector.rs`, `batch.rs`) construct `InputCellView` and `ParsedCell`/`TxView` with owned `Vec<u8>` data. With borrowed types, each helper must:
1. Declare owned data as `let` bindings (e.g., `let lock_hash = vec![...]`)
2. Construct the view struct borrowing those bindings

This is mechanical but increases test verbosity. The practical pattern:

```rust
fn test_example() {
    let lock_hash = vec![0x01; 32];
    let code_hash = vec![0x02; 32];
    let args = vec![0x03; 20];
    let input = InputCellView {
        lock_script_hash: &lock_hash,
        lock_code_hash: &code_hash,
        lock_args: &args,
        // ...
    };
}
```

Verification:
1. **Existing bulk build tests**: `build_history_rows_materializes_*` tests in `mod.rs` cover activity bundle correctness end-to-end.
2. **Protocol detector tests**: Existing tests in all 4 detector files must pass after adapting helpers.
3. **Live sync path**: `build_activity_input_views` compiles and produces identical bundles.
4. **Full sync verification**: Fresh sync + `ckbadger verify --depth fast` validates end-to-end correctness.

## Expected impact

| Metric | Before | After | Change |
|--------|-------:|------:|-------:|
| Activity-related allocs | 1125M | ~201M | -82% |
| history_ms | 157s | ~85-105s | -33% to -46% |
| build_ms | 753s | ~700-725s | -4% to -7% |
| wall clock | 1446s | ~1395-1410s | -2.5% to -3.5% |

Conservative estimate: history_ms is not 100% allocation (serialization, protocol detection, merge, and the deferred `ParsedUdtCell` also contribute). The 82% activity allocation reduction translates to roughly 33-46% history_ms improvement. The wall clock impact is modest because history is one sub-phase of build, which is one stage of the pipeline.

## Files changed

| File | Change |
|------|--------|
| `crates/indexer/src/db/writer/activities.rs` | `InputCellView<'a>`, `OutputCellView<'a>`, `TxView<'a>`, `OwnerAccum<'a>`, `build_tx_activity_bundle`, `classify_input`, `classify_output`, `record_owner_lock_script`, `record_script_call`, `emit_object_changes`, `emit_identity_changes`, test helpers (`make_input`, `make_output`, etc.) |
| `crates/indexer/src/db/writer/rgbpp_detector.rs` | Update `ProtocolDetector::detect` for borrowed `TxView`, update test helpers |
| `crates/indexer/src/db/writer/fiber_detector.rs` | Same |
| `crates/indexer/src/db/writer/stablepp_detector.rs` | Same |
| `crates/indexer/src/db/writer/utxoswap_detector.rs` | Same |
| `crates/indexer/src/sync/bulk_build/mod.rs` | Delete `parsed_cell_from_facts`, `activity_input_view_from_resolved_input`; construct borrowed views directly from facts + interner |
| `crates/indexer/src/sync/batch.rs` | Update `build_activity_input_views` to return borrowed views; update test helpers |
