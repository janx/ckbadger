# Move `created_at_block` from Append-Only to Domain Store

**Date**: 2026-03-12
**Status**: Approved

## Problem

After a depth-1 reorg at block 18819219, the indexer enters an infinite retry loop:

1. Old fork's block 18819219 commits cell payload to append-only CF_CELLS with `created_at_block=N`
2. Reorg rolls back domain store only (append-only is never modified)
3. The transaction moves to a different block on the canonical chain
4. Re-sync writes the same cell with `created_at_block=M` (M != N)
5. Append-only overwrite check detects different value for same key → error
6. Batch cleanup rolls back domain → pipeline resets → same error → infinite loop

Root cause: `LiveCellInfo.created_at_block` is a **position-dependent field** stored in the **append-only** CF_CELLS. The append-only store assumes values are immutable and content-addressed, but `created_at_block` depends on which block includes the transaction, which can change during reorgs.

## Principle Alignment

- **CKB Native**: Cell payloads become truly content-addressed by outpoint
- **Local First**: Re-sync required (cheap per project policy)
- **Fail Fast**: No silent fallbacks; append-only overwrite check stays strict

## Design

### Type Changes

**`LiveCellInfo`** (append-only value) — remove `created_at_block`:

```rust
pub struct LiveCellInfo {
    pub capacity: i64,
    // created_at_block: REMOVED — position-dependent, belongs in domain store
    pub lock_script_hash: Vec<u8>,
    pub lock_code_hash: Vec<u8>,
    pub lock_hash_type: i16,
    pub lock_args: Vec<u8>,
    pub type_script_hash: Option<Vec<u8>>,
    pub type_code_hash: Option<Vec<u8>>,
    pub type_hash_type: Option<i16>,
    pub type_args: Option<Vec<u8>>,
    pub data_size: i32,
    pub occupied_capacity: i64,
    pub udt_amount: Option<u128>,
}
```

**Live cell marker** (domain, CF_LIVE_CELLS) — change from empty `[]` to 8 bytes:

```
value = created_at_block.to_le_bytes()  // i64, 8 bytes LE
```

**`ConsumedCellMeta`** (domain value) — add `created_at_block`:

```rust
pub struct ConsumedCellMeta {
    pub consumed_at_block: i64,
    pub consumed_by_tx: Option<Vec<u8>>,
    pub created_at_block: i64,  // NEW
}
```

The `Cell` common type and all API response types keep `created_at_block` — populated from domain data during read.

### Write Paths

**Cell creation** (`writer/cells.rs::insert_cells_batch`):

- `cells_batch.put_cell_payload(&raw_key, &info)` — `LiveCellInfo` no longer contains `created_at_block`
- `domain_batch.put_live_cell_marker(&raw_key, created_at_block)` — new signature, writes 8 bytes

**Cell consumption** (`writer/cells.rs::consume_cells_batch` / `consume_cells_batch_preloaded`):

- Read `created_at_block` from live marker value (8 bytes LE) instead of from `LiveCellInfo`
- Pass to `put_consumed_cell_meta_raw_key(raw_key, consumed_at_block, consumed_by_tx, created_at_block)`
- Cell index deletion uses `created_at_block` from marker value

Batch commit order unchanged: append-only first, domain second.

### Read Paths

**`get_live_cell_by_outpoint_key`**: Returns `created_at_block` alongside `LiveCellInfo`:

- Read marker value from CF_LIVE_CELLS → decode 8 bytes LE as i64
- Read cell payload from append-only CF_CELLS → deserialize `LiveCellInfo`
- Return both

**`get_consumed_cell_info`**: `created_at_block` comes from `ConsumedCellMeta` instead of `LiveCellInfo`.

**Batch reads** (`get_cells_batch`, `get_consumed_cells_batch`): Decode marker values instead of discarding them.

**API routes**: All reads go through cell_ops methods; conversion to `Cell` / response types populates `created_at_block` from domain data.

### Reorg Rollback

Three rollback paths in `reorg_ops.rs`, all currently read `created_at_block` from `LiveCellInfo` via append-only:

**TX-context path** (`rollback_cells_from_tx_context`):

- Deleting live cells: read `created_at_block` from live marker value. Still read `LiveCellInfo` from append-only for script hashes (index key construction).
- Restoring consumed cells: read `created_at_block` from `ConsumedCellMeta`. Still read `LiveCellInfo` from append-only for script hashes.

**Fallback A** (`delete_live_cells_after_tip_fallback`):

- Decode `created_at_block` from marker value (already in hand from CF_LIVE_CELLS iteration). Still read `LiveCellInfo` from append-only for script hashes.

**Fallback B** (`restore_consumed_cells_fallback`):

- Read `created_at_block` from `ConsumedCellMeta`. Still read `LiveCellInfo` from append-only for script hashes.

**`delete_cell_index_entries` / `put_cell_index_entries`**: Take `created_at_block` as a separate parameter.

### Migration

Re-sync from genesis. Delete both `data/domain/` and `data/append-only/`, restart indexer.

### Testing

- Unit test: live marker round-trip (write 8-byte value, read back `created_at_block`)
- Unit test: `get_live_cell_by_outpoint_key` returns correct `created_at_block` from marker
- Unit test: append-only idempotency — same cell written twice produces identical bytes
- Existing reorg integration test validates full cycle
- Update existing cell_ops tests for new `LiveCellInfo` shape

## Scope

| Crate          | Files                                                                                                                                         | Why                                                             |
| -------------- | --------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------- |
| ckbadger-store | types.rs, batch.rs, cell_ops.rs, reorg_ops.rs                                                                                                 | Type changes, marker value, read/write/rollback ops             |
| indexer        | db/writer/cells.rs, sync/batch.rs, sync/pipeline.rs, sync/reorg.rs                                                                            | Write paths, consumption, HODL tracker                          |
| api            | routes/cells.rs, routes/assets.rs, routes/spore.rs, routes/identities.rs, routes/graph.rs, routes/scripts.rs, routes/statistics.rs, warmup.rs | Read `created_at_block` from domain source                      |
| common         | types/cell.rs                                                                                                                                 | Keep `created_at_block` on `Cell` (populated during conversion) |
