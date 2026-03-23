# Zero-Copy Activity Interface Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate ~1125M heap allocations in history materialization by replacing owned `Vec<u8>` activity interface types with borrowed `&'a [u8]` references.

**Architecture:** Change `InputCellView`, `OwnerAccum`, and `TxView` to carry lifetime `'a` with borrowed `&'a [u8]` fields. Add `OutputCellView<'a>` to replace `&[ParsedCell]`. Both bulk sync (interner) and live sync (LiveCellInfo) paths construct borrowed views cheaply. Only the final `OwnerActivityDelta` conversion calls `.to_vec()`.

**Tech Stack:** Rust (lifetime annotations, zero-copy borrows from interner/existing owned data)

**Spec:** `docs/superpowers/specs/2026-03-23-zero-copy-activity-interface-design.md`

---

## File Map

| File | Role | Change |
|------|------|--------|
| `crates/indexer/src/db/writer/activities.rs` | Activity detection core | Add `OutputCellView<'a>`, change `InputCellView<'a>`, `TxView<'a>`, `OwnerAccum<'a>`, all internal helpers, test helpers |
| `crates/indexer/src/db/writer/rgbpp_detector.rs` | RGBPP protocol detector | Update `detect`/`might_apply` signatures, test helpers |
| `crates/indexer/src/db/writer/fiber_detector.rs` | Fiber protocol detector | Same |
| `crates/indexer/src/db/writer/stablepp_detector.rs` | Stable++ protocol detector | Same |
| `crates/indexer/src/db/writer/utxoswap_detector.rs` | UTXOSwap protocol detector | Same |
| `crates/indexer/src/sync/bulk_build/mod.rs` | Bulk sync history builder | Delete `parsed_cell_from_facts`, `activity_input_view_from_resolved_input`; construct borrowed views |
| `crates/indexer/src/sync/batch.rs` | Live sync writer | Update `build_activity_input_views` + output construction to borrow |

---

### Task 1: Add `OutputCellView<'a>` and make `InputCellView<'a>` borrowed

**Files:**
- Modify: `crates/indexer/src/db/writer/activities.rs:147-176`

This is the foundation. Change the two view structs and `TxView` to use borrowed fields. This will cause compiler errors in all downstream code — that's expected and will be fixed in subsequent tasks.

- [ ] **Step 1: Change `InputCellView` to `InputCellView<'a>`**

Replace the struct at line 149 with borrowed fields:

```rust
/// Input cell info needed for activity building.
#[derive(Clone, Copy)]
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

- [ ] **Step 2: Add `OutputCellView<'a>`**

Add this new struct after `InputCellView`:

```rust
/// Output cell info needed for activity building (borrowed from facts or ParsedCell).
#[derive(Clone, Copy)]
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

- [ ] **Step 3: Update `TxView<'a>`**

Replace at line 167:

```rust
/// Transaction data needed for activity building.
pub struct TxView<'a> {
    pub tx_hash: &'a [u8],
    pub block_hash: &'a [u8],
    pub tx_index: i32,
    pub block_number: i64,
    pub timestamp: i64,
    pub is_cellbase: bool,
    pub inputs: Vec<InputCellView<'a>>,
    pub outputs: Vec<OutputCellView<'a>>,
}
```

- [ ] **Step 4: Verify types compile (expect downstream errors)**

Run: `cargo check -p ckbadger-indexer 2>&1 | head -50`

Expected: Errors in `build_tx_activity_bundle`, detectors, batch.rs, mod.rs — all expected. The type definitions themselves should compile.

---

### Task 2: Make `OwnerAccum<'a>` and `build_tx_activity_bundle` borrowed

**Files:**
- Modify: `crates/indexer/src/db/writer/activities.rs:246-633`

Change `OwnerAccum` to hold borrowed references and update the main activity bundle builder.

- [ ] **Step 1: Change `OwnerAccum` to `OwnerAccum<'a>`**

Replace the struct at line 248:

```rust
#[derive(Default)]
pub(crate) struct OwnerAccum<'a> {
    pub(crate) lock_code_hash: Option<&'a [u8]>,
    pub(crate) lock_hash_type: Option<i16>,
    pub(crate) lock_args: Option<&'a [u8]>,
    pub(crate) input_capacity: i128,
    pub(crate) output_capacity: i128,
    pub(crate) input_used: i64,
    pub(crate) output_used: i64,
    pub(crate) udt_deltas: HashMap<&'a [u8], (i128, i128)>,
    pub(crate) dao_deposits: Vec<i64>,
    pub(crate) dao_withdraw_requests: Vec<(i64, i64)>,
    pub(crate) dao_withdraw_completes: Vec<(i64, i64)>,
    pub(crate) spore_inputs: Vec<&'a [u8]>,
    pub(crate) spore_outputs: Vec<&'a [u8]>,
    pub(crate) nft_inputs: Vec<&'a [u8]>,
    pub(crate) nft_outputs: Vec<&'a [u8]>,
    pub(crate) dotbit_inputs: Vec<Vec<u8>>,
    pub(crate) dotbit_outputs: Vec<Vec<u8>>,
    pub(crate) did_ckb_inputs: Vec<&'a [u8]>,
    pub(crate) did_ckb_outputs: Vec<&'a [u8]>,
    pub(crate) involved_scripts: BTreeSet<&'a [u8]>,
    pub(crate) has_type_script: bool,
    pub(crate) unrecognized_type_calls: BTreeSet<(&'a [u8], i16, &'a [u8])>,
    pub(crate) unrecognized_lock_calls: BTreeSet<(&'a [u8], i16, &'a [u8])>,
}
```

- [ ] **Step 2: Update `record_owner_lock_script`**

At line 294, add lifetime and change from `Vec<u8>` comparisons to `&[u8]`:

```rust
fn record_owner_lock_script<'a>(
    accum: &mut OwnerAccum<'a>,
    lock_code_hash: &'a [u8],
    lock_hash_type: i16,
    lock_args: &'a [u8],
) -> Result<()> {
    match (
        accum.lock_code_hash,
        accum.lock_hash_type,
        accum.lock_args,
    ) {
        (Some(existing_code_hash), Some(existing_hash_type), Some(existing_args)) => {
            if existing_code_hash != lock_code_hash {
                bail!(
                    "owner lock_code_hash mismatch for same lock hash: existing=0x{}, new=0x{}",
                    hex::encode(existing_code_hash),
                    hex::encode(lock_code_hash)
                );
            }
            if existing_hash_type != lock_hash_type {
                bail!(
                    "owner lock_hash_type mismatch for same lock hash: existing={}, new={}, lock_code_hash=0x{}",
                    existing_hash_type,
                    lock_hash_type,
                    hex::encode(lock_code_hash)
                );
            }
            if existing_args != lock_args {
                bail!(
                    "owner lock_args mismatch for same lock hash: existing_len={}, new_len={}, lock_code_hash=0x{}",
                    existing_args.len(),
                    lock_args.len(),
                    hex::encode(lock_code_hash)
                );
            }
        }
        (None, None, None) => {
            accum.lock_code_hash = Some(lock_code_hash);
            accum.lock_hash_type = Some(lock_hash_type);
            accum.lock_args = Some(lock_args);
        }
        _ => bail!(
            "owner lock script state partially initialized: code_hash={}, hash_type={}, args={}",
            accum.lock_code_hash.is_some(),
            accum.lock_hash_type.is_some(),
            accum.lock_args.is_some()
        ),
    }
    Ok(())
}
```

- [ ] **Step 3: Update `build_tx_activity_bundle` signature and owners HashMap**

Change function signature and the owners map to use borrowed keys. Update input/output processing loops to use `OutputCellView`/`InputCellView` fields (which are now `&[u8]`). Key changes:

- `let mut owners: HashMap<&'a [u8], OwnerAccum<'a>> = HashMap::new();`
- Input loop: `owners.entry(input.lock_script_hash)` (no `.clone()`)
- `accum.involved_scripts.insert(input.lock_code_hash)` (no `.clone()`)
- Output loop: `for cell in &tx.outputs` (was `for cell in tx.outputs` on slice, now `&` needed for Vec)
- `owners.entry(cell.lock_script_hash)` (no `.clone()`)
- Output lock detection loop: `for cell in &tx.outputs` — compare `&[u8]` directly
- Non-standard lock recording: `accum.unrecognized_lock_calls.insert((cell.lock_code_hash, cell.lock_hash_type, cell.lock_args))` (no `.clone()`)
- Owner hashes collection: `let mut owner_hashes: Vec<&[u8]> = owners.keys().copied().collect();`
- Peers: `owners.keys().filter(...).copied().collect()`
- `token_info_cache.get(*type_script_hash)` — dereference `&&[u8]` to `&[u8]` for HashMap lookup
- Simplify `cell.type_args.as_ref().map(|a| a.len())` → `cell.type_args.map(|a| a.len())` (`.as_ref()` redundant on `Option<&[u8]>`)

- [ ] **Step 4: Update final `OwnerActivityDelta` conversion**

The conversion from `OwnerAccum<'a>` → `OwnerActivityDelta` (owned, for DB) is where `.to_vec()` now happens:

```rust
bundle_owners.push(OwnerActivityDelta {
    lock_hash: lock_hash.to_vec(),
    lock_code_hash: accum.lock_code_hash.ok_or_else(|| ...)?.to_vec(),
    lock_hash_type: accum.lock_hash_type.ok_or_else(|| ...)?,
    lock_args: accum.lock_args.ok_or_else(|| ...)?.to_vec(),
    ckb_delta,
    used_delta,
    has_type_script: accum.has_type_script,
    involved_script_code_hashes: accum.involved_scripts.iter().map(|s| s.to_vec()).collect(),
    asset_changes,
    type_calls,
    lock_calls,
    protocol_actions,
    peers: peers.into_iter().map(|p| p.to_vec()).collect(),
});
```

- [ ] **Step 5: Verify `activities.rs` core compiles (expect downstream errors)**

Run: `cargo check -p ckbadger-indexer 2>&1 | head -80`

Expected: Errors in classify/emit helpers, detectors, test helpers, batch.rs, mod.rs.

---

### Task 3: Update internal helper functions in `activities.rs`

**Files:**
- Modify: `crates/indexer/src/db/writer/activities.rs:662-915`

Propagate lifetimes through `classify_input`, `classify_output`, `record_script_call`, `emit_object_changes`, `emit_identity_changes`.

- [ ] **Step 1: Update `classify_input`**

Add lifetime `'a` to parameters that flow into `OwnerAccum<'a>` storage. Change `.to_vec()` calls to direct borrow stores:

```rust
fn classify_input<'a>(
    accum: &mut OwnerAccum<'a>,
    type_code_hash: &'a [u8],
    type_hash_type: Option<i16>,
    type_script_hash: Option<&'a [u8]>,
    type_args: Option<&'a [u8]>,
    udt_amount: Option<u128>,
    data: &'a [u8],
    is_dao_withdraw_request: bool,
    dao_compensation: Option<i64>,
    hashes: &CodeHashes,
    capacity: i64,
) -> Result<()> {
```

Inside the function body, change:
- `accum.udt_deltas.entry(tsh.to_vec())` → `accum.udt_deltas.entry(tsh)`
- `accum.did_ckb_inputs.push(args.to_vec())` → `accum.did_ckb_inputs.push(args)`
- `accum.spore_inputs.push(args.to_vec())` → `accum.spore_inputs.push(args)`
- `accum.nft_inputs.push(args.to_vec())` → `accum.nft_inputs.push(args)`
- `accum.dotbit_inputs.push(account_id)` stays (already `Vec<u8>`, computed)

- [ ] **Step 2: Update `classify_output`**

Same pattern as `classify_input`:

```rust
fn classify_output<'a>(
    accum: &mut OwnerAccum<'a>,
    type_code_hash: &'a [u8],
    type_hash_type: Option<i16>,
    type_script_hash: Option<&'a [u8]>,
    type_args: Option<&'a [u8]>,
    cell_data: &'a [u8],
    hashes: &CodeHashes,
    capacity: i64,
) -> Result<()> {
```

Change `.to_vec()` → direct borrow for spore_outputs, nft_outputs, did_ckb_outputs. dotbit_outputs stays owned.

- [ ] **Step 3: Update `record_script_call`**

```rust
fn record_script_call<'a>(
    accum: &mut OwnerAccum<'a>,
    type_code_hash: &'a [u8],
    type_hash_type: Option<i16>,
    type_args: Option<&'a [u8]>,
) -> Result<()> {
    let hash_type = type_hash_type.ok_or_else(|| { ... })?;
    let args = type_args.ok_or_else(|| { ... })?;
    accum.unrecognized_type_calls.insert((type_code_hash, hash_type, args));
    Ok(())
}
```

No more `.to_vec()` — store borrowed references directly.

- [ ] **Step 4: Update `emit_object_changes` and `emit_identity_changes`**

Change parameter types from `&[Vec<u8>]` to generic:

```rust
fn emit_object_changes<T: AsRef<[u8]>>(
    inputs: &[T],
    outputs: &[T],
    standard: &str,
    asset_changes: &mut Vec<AssetChange>,
) {
    for id in outputs {
        let id = id.as_ref();
        let in_inputs = inputs.iter().any(|i| i.as_ref() == id);
        let action = if in_inputs { AssetAction::Transfer } else { AssetAction::Mint };
        asset_changes.push(AssetChange::Object {
            object_id: id.to_vec(),
            standard: standard.to_string(),
            action,
        });
    }
    for id in inputs {
        let id = id.as_ref();
        let in_outputs = outputs.iter().any(|o| o.as_ref() == id);
        if !in_outputs {
            asset_changes.push(AssetChange::Object {
                object_id: id.to_vec(),
                standard: standard.to_string(),
                action: AssetAction::Burn,
            });
        }
    }
}
```

Same pattern for `emit_identity_changes`. This keeps both `&[Vec<u8>]` (dotbit, stays owned) and `&[&[u8]]` (spore/nft/did_ckb) working.

- [ ] **Step 5: Update call sites in `build_tx_activity_bundle`**

The calls to `classify_input` and `classify_output` from `build_tx_activity_bundle` now pass fields from `InputCellView<'a>` and `OutputCellView<'a>`:

For inputs (was `&input.lock_code_hash` on `Vec<u8>`, now `input.lock_code_hash` on `&'a [u8]`):
```rust
classify_input(
    accum,
    input.type_code_hash.unwrap(),  // &'a [u8] directly
    input.type_hash_type,
    input.type_script_hash,  // Option<&'a [u8]> directly
    input.type_args,
    input.udt_amount,
    input.data,
    input.is_dao_withdraw_request,
    input.dao_compensation,
    hashes,
    input.capacity,
)?;
```

For outputs (accessing `OutputCellView<'a>` fields):
```rust
classify_output(
    accum,
    cell.type_code_hash.unwrap(),
    cell.type_hash_type,
    cell.type_script_hash,
    cell.type_args,
    cell.data,
    hashes,
    cell.capacity,
)?;
```

- [ ] **Step 6: Verify activities.rs compiles (expect only external errors)**

Run: `cargo check -p ckbadger-indexer 2>&1 | head -80`

Expected: Errors only in detectors, batch.rs, mod.rs, and test code.

---

### Task 4: Update `ProtocolDetector` trait and all 4 detectors

**Files:**
- Modify: `crates/indexer/src/db/writer/activities.rs:179-205` (trait)
- Modify: `crates/indexer/src/db/writer/rgbpp_detector.rs`
- Modify: `crates/indexer/src/db/writer/fiber_detector.rs`
- Modify: `crates/indexer/src/db/writer/stablepp_detector.rs`
- Modify: `crates/indexer/src/db/writer/utxoswap_detector.rs`

- [ ] **Step 1: Update `ProtocolDetector` trait**

The `detect` method receives `&OwnerAccum` which now has lifetime. Update:

```rust
fn detect(
    &self,
    tx: &TxView<'_>,
    owner_lock_hash: &[u8],
    accum: &OwnerAccum<'_>,
    asset_changes: &[AssetChange],
    type_calls: &[TypeCallEntry],
    lock_calls: &[LockCallEntry],
) -> Vec<ProtocolAction>;
```

The `might_apply` method already takes `&TxView<'_>` — its body accesses `tx.outputs` and `tx.inputs` fields. With `OutputCellView<'a>`, field access patterns change:
- Was: `cell.type_code_hash.as_ref().map(|v| v.as_slice())` → Now: `cell.type_code_hash` (already `Option<&[u8]>`)
- Was: `cell.lock_code_hash.as_slice()` → Now: `cell.lock_code_hash` (already `&[u8]`)

- [ ] **Step 2: Update `rgbpp_detector.rs`**

The `TypeGroupCell` struct (line 18) stores `lock_script_hash: Vec<u8>` and `lock_args: Vec<u8>`, populated via `.clone()` from what are now `&[u8]` fields. Change `.clone()` to `.to_vec()` at those sites (keeping `TypeGroupCell` owned — it's a local temporary in `detect()`). Also:
- `for output in tx.outputs` → `for output in &tx.outputs`
- `for input in &tx.inputs` stays (already borrows)
- Simplify `output.type_code_hash.as_ref()` → `output.type_code_hash` (already `Option<&[u8]>`)
- Simplify `output.type_args.as_ref()` → `output.type_args`

- [ ] **Step 3: Update `fiber_detector.rs`**

`FiberCellSummary` (line 35) stores `Vec<Vec<u8>>` and `Option<Vec<u8>>` populated via `.clone()` from `InputCellView`/`OutputCellView` fields. Change `.clone()` to `.to_vec()` at those sites (keeping `FiberCellSummary` owned — it's a local temporary in `detect()`). Also:
- `for output in tx.outputs` → `for output in &tx.outputs`
- Simplify redundant `.as_ref()` / `.as_slice()` chains on now-borrowed fields

- [ ] **Step 4: Update `stablepp_detector.rs`**

Focus on `has_stablepp_scripts` and `might_apply` which check code hashes. Change:
- `for output in tx.outputs` → `for output in &tx.outputs`
- `input.type_code_hash.as_ref().is_some_and(...)` → `input.type_code_hash.is_some_and(...)`
- Same simplification for output fields

- [ ] **Step 5: Update `utxoswap_detector.rs`**

The detector reads `accum.output_capacity` and `accum.input_capacity` (scalars, unchanged) plus `tx.outputs/inputs` fields. Change:
- `for output in tx.outputs` → `for output in &tx.outputs`
- Simplify `.as_ref()` chains on borrowed fields

- [ ] **Step 6: Verify all detectors compile**

Run: `cargo check -p ckbadger-indexer 2>&1 | grep -c "error"`

Expected: Errors only in batch.rs, mod.rs, and test code.

---

### Task 5: Update bulk sync path (`mod.rs`)

**Files:**
- Modify: `crates/indexer/src/sync/bulk_build/mod.rs:2627-2710` (activity bundle construction)
- Delete: `crates/indexer/src/sync/bulk_build/mod.rs` functions `parsed_cell_from_facts` (~line 3311) and `activity_input_view_from_resolved_input` (~line 3255)

- [ ] **Step 1: Replace `parsed_cell_from_facts` with inline `OutputCellView` construction**

In `build_history_rows_for_block`, the activity bundle section (around line 2639-2710) currently constructs `block_outputs` via `parsed_cell_from_facts`. Replace with:

```rust
block_outputs.push(
    resolved_tx
        .cells
        .iter()
        .map(|cell| OutputCellView {
            capacity: cell.capacity,
            lock_code_hash: interner.resolve_bytes(cell.lock_code_hash_id),
            lock_hash_type: cell.lock_hash_type,
            lock_args: interner.resolve_bytes(cell.lock_args_id),
            lock_script_hash: interner.resolve_bytes(cell.lock_script_hash_id),
            type_code_hash: cell.type_code_hash_id.map(|id| interner.resolve_bytes(id)),
            type_hash_type: cell.type_hash_type,
            type_args: cell.type_args_id.map(|id| interner.resolve_bytes(id)),
            type_script_hash: cell.type_script_hash_id.map(|id| interner.resolve_bytes(id)),
            data_hash: cell.data_hash.as_ref().map_or(&[], |h| h.as_slice()),
            data_size: cell.data_size,
            data: &cell.data,
        })
        .collect::<Vec<_>>(),
);
```

Note: `data_hash` on `CellFacts` is `Option<[u8; 32]>`. For the activity view we provide `&[u8]` — when None, use empty slice. The `ScriptParser::compute_data_hash` call used in `parsed_cell_from_facts` is NOT needed here because activity detection doesn't use `data_hash`.

- [ ] **Step 2: Replace `activity_input_view_from_resolved_input` with inline `InputCellView` construction**

```rust
block_inputs.push(
    resolved_tx
        .resolved_inputs
        .iter()
        .map(|input| -> Result<InputCellView<'_>> {
            let (is_dao_withdraw_request, dao_compensation) = match (
                input.dao_state,
                input.dao_compensation_ars,
            ) {
                (
                    Some(facts::DaoCellState::WithdrawRequest { .. }),
                    Some(facts::DaoCompensationArs { deposit_ar, withdraw_request_ar }),
                ) => (
                    true,
                    Some(crate::db::writer::dao::calculate_dao_compensation_from_ar(
                        input.capacity, deposit_ar, withdraw_request_ar,
                    )?),
                ),
                (Some(facts::DaoCellState::WithdrawRequest { .. }), None) => {
                    bail!("missing DAO compensation ARs for input: outpoint=0x{}:{}",
                        hex::encode(input.outpoint.tx_hash), input.outpoint.index);
                }
                _ => (false, None),
            };
            Ok(InputCellView {
                lock_script_hash: interner.resolve_bytes(input.lock_script_hash_id),
                lock_code_hash: interner.resolve_bytes(input.lock_code_hash_id),
                lock_hash_type: input.lock_hash_type,
                lock_args: interner.resolve_bytes(input.lock_args_id),
                capacity: input.capacity,
                occupied_capacity: input.occupied_capacity,
                type_code_hash: input.type_code_hash_id.map(|id| interner.resolve_bytes(id)),
                type_hash_type: input.type_hash_type,
                type_script_hash: input.type_script_hash_id.map(|id| interner.resolve_bytes(id)),
                type_args: input.type_args_id.map(|id| interner.resolve_bytes(id)),
                udt_amount: input.udt_amount,
                data: &[],
                is_dao_withdraw_request,
                dao_compensation,
            })
        })
        .collect::<Result<Vec<_>>>()?,
);
```

- [ ] **Step 3: Update `TxView` construction**

The `TxView` construction around line 2673-2689 needs to change `outputs` from `&td.cells` slice to the new `Vec<OutputCellView>`:

```rust
let tx_views = block_txs
    .iter()
    .zip(block_inputs)
    .zip(block_outputs)
    .map(
        |((tx, inputs), outputs)| crate::db::writer::activities::TxView {
            tx_hash: &tx.hash,
            block_hash: &tx.block_hash,
            tx_index: tx.tx_index,
            block_number: tx.block_number,
            timestamp: tx.timestamp_ms,
            is_cellbase: tx.is_cellbase,
            inputs,
            outputs,
        },
    )
    .collect::<Vec<_>>();
```

- [ ] **Step 4: Delete `parsed_cell_from_facts` and `activity_input_view_from_resolved_input`**

These functions are no longer called. Delete them.

- [ ] **Step 5: Verify bulk sync path compiles**

Run: `cargo check -p ckbadger-indexer 2>&1 | grep -c "error"`

Expected: Errors only in batch.rs and test code.

- [ ] **Step 6: Commit**

```
git add -A && git commit -m "feat(bulk-build): zero-copy activity views from interner

Replace parsed_cell_from_facts and activity_input_view_from_resolved_input
with inline OutputCellView/InputCellView construction that borrows directly
from the interner. Eliminates ~1049M .to_vec() allocations in bulk sync."
```

---

### Task 6: Update live sync path (`batch.rs`)

**Files:**
- Modify: `crates/indexer/src/sync/batch.rs:146-215` (`build_activity_input_views`)
- Modify: `crates/indexer/src/sync/batch.rs:2490-2512` (TxView construction)

- [ ] **Step 1: Update `build_activity_input_views` return type and body**

Change to return borrowed views from the existing `PositionedCellInfo`:

```rust
fn build_activity_input_views<'a>(
    tx_data: &TxData,
    block_number: i64,
    input_cell_info: &'a HashMap<(Vec<u8>, i16), PositionedCellInfo>,
    batch_cell_infos: &'a HashMap<(Vec<u8>, i16), PositionedCellInfo>,
    dao_withdraw_outpoints: &HashSet<(Vec<u8>, i16)>,
    dao_compensations: &HashMap<(Vec<u8>, i16), i64>,
) -> Result<Vec<crate::db::writer::activities::InputCellView<'a>>> {
```

Inside, replace `.clone()` with borrows:

```rust
Ok(crate::db::writer::activities::InputCellView {
    lock_script_hash: &info.lock_script_hash,
    lock_code_hash: &info.lock_code_hash,
    lock_hash_type: info.lock_hash_type,
    lock_args: &info.lock_args,
    capacity: info.capacity,
    occupied_capacity: info.occupied_capacity,
    type_code_hash: info.type_code_hash.as_deref(),
    type_hash_type: info.type_hash_type,
    type_script_hash: info.type_script_hash.as_deref(),
    type_args: info.type_args.as_deref(),
    udt_amount: info.udt_amount,
    data: &[],
    is_dao_withdraw_request,
    dao_compensation,
})
```

- [ ] **Step 2: Update output construction in block processing loop**

Around line 2490-2512, change the `TxView` construction to build `Vec<OutputCellView>` from `ParsedCell`:

```rust
let outputs: Vec<crate::db::writer::activities::OutputCellView<'_>> = td.cells
    .iter()
    .map(|cell| crate::db::writer::activities::OutputCellView {
        capacity: cell.capacity,
        lock_code_hash: &cell.lock_code_hash,
        lock_hash_type: cell.lock_hash_type,
        lock_args: &cell.lock_args,
        lock_script_hash: &cell.lock_script_hash,
        type_code_hash: cell.type_code_hash.as_deref(),
        type_hash_type: cell.type_hash_type,
        type_args: cell.type_args.as_deref(),
        type_script_hash: cell.type_script_hash.as_deref(),
        data_hash: &cell.data_hash,
        data_size: cell.data_size,
        data: &cell.data,
    })
    .collect();

Ok(crate::db::writer::activities::TxView {
    tx_hash: &td.hash,
    block_hash: &parsed.hash,
    tx_index: td.tx_index,
    block_number: parsed.number,
    timestamp: parsed.timestamp.timestamp_millis(),
    is_cellbase: td.is_cellbase,
    inputs,
    outputs,
})
```

- [ ] **Step 3: Verify live sync path compiles**

Run: `cargo check -p ckbadger-indexer 2>&1 | grep -c "error"`

Expected: Errors only in test code.

- [ ] **Step 4: Commit**

```
git add -A && git commit -m "feat(batch): zero-copy activity views from LiveCellInfo

Update live sync path to borrow from existing PositionedCellInfo/ParsedCell
instead of cloning Vec<u8> fields into InputCellView/OutputCellView."
```

---

### Task 7: Update all test helpers

**Files:**
- Modify: `crates/indexer/src/db/writer/activities.rs` (test module, ~line 917+: `make_output`, `make_input`, `make_input_with_lock`, `make_output_with_lock`, and all test functions)
- Modify: `crates/indexer/src/db/writer/fiber_detector.rs` (test helpers: `make_input_with_lock`, `make_output_with_lock`)
- Modify: `crates/indexer/src/db/writer/stablepp_detector.rs` (test helpers: `make_input`, `make_output`)
- Modify: `crates/indexer/src/db/writer/utxoswap_detector.rs` (test helpers: `make_input`, `make_output`)
- Modify: `crates/indexer/src/sync/batch.rs` (3 test functions constructing `InputCellView`)
- Note: `rgbpp_detector.rs` has no test module — no changes needed

Test helpers need to declare owned data as `let` bindings, then construct views that borrow from them. The pattern is consistent across all files.

- [ ] **Step 1: Update `activities.rs` test helpers**

`make_output` returns `ParsedCell` — it's used for constructing `OutputCellView` in tests. Change it to return owned data plus an `OutputCellView` construction helper:

```rust
fn make_output_view<'a>(
    lock_hash: &'a [u8],
    lock_code_hash: &'a [u8],
    lock_args: &'a [u8],
    capacity: i64,
    type_code_hash: Option<&'a [u8]>,
    type_script_hash: Option<&'a [u8]>,
    type_args: Option<&'a [u8]>,
    data: &'a [u8],
) -> OutputCellView<'a> {
    OutputCellView {
        capacity,
        lock_code_hash,
        lock_hash_type: 1,
        lock_args,
        lock_script_hash: lock_hash,
        type_code_hash,
        type_hash_type: None,
        type_args,
        type_script_hash,
        data_hash: &[],
        data_size: data.len() as i32,
        data,
    }
}

fn make_input_view<'a>(
    lock_hash: &'a [u8],
    lock_code_hash: &'a [u8],
    lock_args: &'a [u8],
    capacity: i64,
    occupied: i64,
) -> InputCellView<'a> {
    InputCellView {
        lock_script_hash: lock_hash,
        lock_code_hash,
        lock_hash_type: 1,
        lock_args,
        capacity,
        occupied_capacity: occupied,
        type_code_hash: None,
        type_hash_type: None,
        type_script_hash: None,
        type_args: None,
        udt_amount: None,
        data: &[],
        is_dao_withdraw_request: false,
        dao_compensation: None,
    }
}
```

Then update each test to declare owned byte arrays and call the helpers:

```rust
#[test]
fn test_example() {
    let lock_hash = [0xAA_u8; 32];
    let code_hash = [0x11_u8; 32];
    let args = [0x22_u8; 20];
    let input = make_input_view(&lock_hash, &code_hash, &args, 100_00000000, 61_00000000);
    // ...
}
```

- [ ] **Step 2: Update detector test helpers**

Each detector file has `make_input`/`make_output`/`make_input_with_lock`/`make_output_with_lock` helpers. Apply the same pattern: owned data as constants or `let` bindings, pass as `&[u8]` to constructors.

- [ ] **Step 3: Update `batch.rs` test helpers**

Same pattern for the 3 test functions that construct `InputCellView`.

- [ ] **Step 4: Run all tests**

Run: `cargo test -p ckbadger-indexer -- --lib 2>&1 | tail -20`

Expected: All tests pass.

- [ ] **Step 5: Commit**

```
git add -A && git commit -m "test: update activity test helpers for borrowed views

Adapt all test helper functions to declare owned byte data as let bindings
and construct InputCellView/OutputCellView by borrowing from them."
```

---

### Task 8: Final verification

**Files:** None (verification only)

- [ ] **Step 1: Full type check**

Run: `cargo check && cargo clippy`

Expected: Clean.

- [ ] **Step 2: Run all Rust tests**

Run: `cargo test`

Expected: All pass.

- [ ] **Step 3: Run frontend checks** (sanity — no frontend changes)

Run: `cd frontend && pnpm type-check && pnpm lint`

Expected: Clean.

- [ ] **Step 4: Squash or tidy commits if needed**

Review commit history and ensure clean progression.
