# Append-Only History Indices Design

## Goal

- Move `activities`, `addr_txs`, and `nft_collection_activities` onto the append-only store.
- Preserve orphaned history across reorgs while keeping API responses canonical by default.
- Improve write-path isolation by moving append-heavy historical indexes out of the mutable domain RocksDB.

## Principle Alignment

- CKB Native: canonical truth remains derived from chain position in the domain store; immutable historical observations live in append-only archives.
- Local First: rebuild-from-genesis remains the primary migration path; no backfill or compatibility layer is required.
- Agent Friendly: one explicit rule defines storage responsibility: immutable history indexes live in append-only, canonical projections and aggregates live in domain.

## Problem Summary

- `docs/prompts/ACTIVITY_SYSTEM.md` and `docs/prompts/DATA_DESIGN.md` treat `activities` as append-only historical data.
- Current implementation keeps `CF_ACTIVITIES` in `DOMAIN_CFS` and writes activity batches into the domain store.
- `addr_txs` and `nft_collection_activities` already live in append-only, but all three history indexes use canonical block/tx position as part of the key shape.
- Canonical-position keys are not reorg-safe for append-only archives: the same address or collection can appear at the same block/tx position on two different forks, causing append-only key collisions.

## Decision

- Treat `activities`, `addr_txs`, and `nft_collection_activities` as append-only historical indexes.
- Preserve orphaned history permanently in append-only storage.
- Keep API default behavior canonical-only by filtering append-only rows through the domain store's current canonical tx location.
- Re-key all three indexes so append-only uniqueness does not depend on canonical position alone.

## Naming Clarification

- `domain knowledge` and `domain store` are not the same concept.
- `activities` remain domain knowledge in the information architecture sense.
- `domain store` means mutable latest canonical/query state.
- `append-only store` means immutable history/archive state.
- Domain knowledge may live in append-only storage when the stored record is immutable history and that layout is better for write throughput and reorg handling.

## Constraints

- No compatibility path or in-place repair is needed; full rebuild is acceptable.
- No fallback calculation chains may be added.
- Append-only history must never be updated or deleted during reorg handling.
- Canonical API views must continue to return newest-first canonical rows only.
- Existing append-only write validation must remain strict and fail fast on unexpected overwrites.

## Approaches Considered

### Approach A: TxHash-Anchored Append-Only Indexes

- Keep the current entity-scoped sorted indexes, but add `tx_hash` to the key so fork-local duplicates do not collide.
- Continue using the domain store's `tx_hash_map` and `tx_index` as the single canonical filter.

Trade-offs:

- Simple and directly compatible with existing reader shape.
- Preserves newest-first scans.
- Requires key migration and re-sync, but no new global ID allocator.

### Approach B: Dual-Key Hybrid

- Keep the old canonical-position keys for readers and add a second append-safe archive key.

Trade-offs:

- Lower short-term migration risk.
- Violates single-calculation-path and single-source-of-truth principles.
- Creates long-term duplication and drift risk.

### Approach C: Global Event IDs

- Introduce a new per-event ID and make all history indexes point to it.

Trade-offs:

- Most general.
- Highest complexity.
- Unnecessary for the current problem.

## Recommendation

- Use Approach A.
- The existing reader behavior already assumes "scan history rows, then canonical-filter them".
- Adding `tx_hash` to the key is the minimal change that makes append-only uniqueness reorg-safe while preserving scan order and API semantics.

## Proposed Storage Model

### 1. `activities`

- Move `CF_ACTIVITIES` from `DOMAIN_CFS` to `APPEND_CFS`.
- New key:

```text
lock_hash(32) + block_num_desc(8) + tx_idx_desc(4) + tx_hash(32)
```

- Value remains `ActivityEntry`.

Properties:

- Prefix scans still paginate newest-first per address.
- Two different transactions on two forks at the same canonical position no longer collide.
- Canonical filtering still uses `entry.tx_hash`.

### 2. `addr_txs`

- Keep `CF_ADDR_TXS` in `APPEND_CFS`.
- New key:

```text
lock_hash(32) + block_num_desc(8) + tx_idx_desc(4) + tx_hash(32)
```

- Value remains `tx_hash` for simple read-path usage.

Properties:

- Same reorg-safety as `activities`.
- Existing address transaction pagination behavior remains intact after canonical filtering.

### 3. `nft_collection_activities`

- Keep `CF_NFT_COLLECTION_ACTIVITIES` in `APPEND_CFS`.
- New key:

```text
collection_id(32) + block_num_desc(8) + tx_idx_desc(4) + tx_hash(32)
```

- Value remains `NftCollectionActivityEntry`.

Properties:

- Prefix scans remain newest-first per collection.
- Orphaned collection activity history can coexist with new canonical history.

## Canonical Read Model

### Shared Rule

- Append-only store is the source of historical candidates.
- Domain store is the source of canonical truth.
- A row is canonical only if:
  - its `tx_hash` exists in `tx_hash_map`, and
  - the current canonical `(block_num, tx_idx)` for that `tx_hash` matches the position encoded in the append-only key.

### Address Activities

- Scan append-only `activities`.
- Use `ActivityEntry.tx_hash` for canonical validation against domain `tx_hash_map`.
- Skip orphaned rows silently.

### Address Transactions

- Scan append-only `addr_txs`.
- Validate the returned `tx_hash` against domain `tx_hash_map` and `tx_index`.
- Skip orphaned rows silently.

### NFT / Spore Collection Activities

- Scan append-only `nft_collection_activities`.
- Validate `entry.tx_hash` against domain `tx_hash_map`.
- Skip orphaned rows silently.

### Aggregates

- Mutable aggregates such as `addr_balance.txs_count` and `nft_collection_agg.activities_count` remain in the domain store.
- Their values must continue to represent canonical state only.
- On rebuild or rollback repair, canonical counts are recomputed by scanning append-only history and applying the same canonical filter.

## Reorg Semantics

### Append-Only History

- Reorg never deletes:
  - `activities`
  - `addr_txs`
  - `nft_collection_activities`

- Reorg only changes domain canonical state.

### Domain State

- `rollback_to_block()` continues to remove or restore mutable canonical projections:
  - block headers
  - tx position maps
  - live sets
  - mutable aggregates

- Repair paths that rebuild aggregates from history must use canonical filtering over append-only rows.

### Undo Log

- Forward writes for the three history indexes record append-target undo entries.
- Rollback replay prunes those undo-log entries only.
- No append-store delete or overwrite path is introduced.

### Documentation Impact

- `docs/prompts/REORG_HANDLING.md` must stop describing `activities` as a rollback-deleted core CF.
- It should instead state:
  - domain canonical projections are rolled back,
  - append-only history indexes are preserved,
  - canonical API views are rebuilt or filtered from preserved history.

## Performance Assessment

## Why append-only is likely better

- `CF_ACTIVITIES` is already treated in code as:
  - a mega-write CF,
  - a high-write CF,
  - and a historical append-heavy CF.

- Current domain RocksDB uses:
  - `atomic_flush=true`,
  - one global `WriteBufferManager`,
  - shared background jobs across all CFs in the same DB.

- Leaving `activities` in the domain store means:
  - its append-heavy write load shares flush and compaction pressure with mutable CFs such as `live_cells`, `tx_index`, `addr_balance`, and stats.

- Moving it to append-only isolates that pressure into the history/archive DB where append-heavy CF tuning already matches the workload.

## Costs

- Keys become 32 bytes larger because `tx_hash` is added.
- Range scans read slightly more key bytes.
- Canonical filtering still requires domain lookups.

## Net Judgment

- The expected write-path improvement comes mainly from RocksDB-instance isolation, not from the longer key itself.
- Given the current tuning and batch structure, the migration is likely a net write-throughput win.
- This is a reasoned engineering judgment, not a benchmark claim; implementation should include before/after measurements if the migration lands.

## Affected Files

- `crates/ckbadger-store/src/store.rs`
  - move `CF_ACTIVITIES` to append-only ownership
  - update append-only CF handle resolution
- `crates/ckbadger-store/src/keys.rs`
  - re-key `activities`, `addr_txs`, and `nft_collection_activities`
  - add new decode helpers as needed
- `crates/ckbadger-store/src/activity_ops.rs`
  - read new activity keys
- `crates/ckbadger-store/src/address_ops.rs`
  - read new addr_txs keys
- `crates/ckbadger-store/src/nft_ops.rs`
  - read new collection activity keys
- `crates/indexer/src/sync/batch.rs`
  - write activity history to append-only store in both bulk and live sync paths
- `crates/indexer/src/sync/undo.rs`
  - mark activity undo entries as append-only
- `crates/ckbadger-store/src/reorg_ops.rs`
  - rebuild canonical aggregates from append-only history with canonical filtering
- `crates/api/src/routes/activities.rs`
  - keep append-only source and adapt to new key decoding
- `crates/api/src/routes/cells.rs`
  - adapt `addr_txs` scans to new key shape
- `crates/api/src/routes/assets.rs`
  - adapt collection activity scans to new key shape
- `docs/prompts/ACTIVITY_SYSTEM.md`
  - align storage description with final implementation
- `docs/prompts/REORG_HANDLING.md`
  - align rollback semantics
- `docs/STORE_SCHEMA.md`
  - align CF placement and purpose
- `docs/prompts/INFORMATION_DESIGN.md`
  - optional wording clarification on information layer vs store responsibility

## Validation Strategy

### Store-Level

- Add regression tests proving append-only history accepts:
  - same address / collection,
  - same block and tx index,
  - different `tx_hash`,
  - without overwrite failure.

- Add tests proving append-only deletion remains forbidden.

### API-Level

- Add canonical-filter tests proving orphaned rows remain stored but are not returned from:
  - address activities,
  - address transactions,
  - collection activities.

- Remove reliance on `open_test_unified()` for split-sensitive routes.

### Indexer-Level

- Add reorg tests proving:
  - append-only history rows survive rollback,
  - domain aggregates and visible API results reflect only canonical rows.

### Verification Commands

- `cargo test -p ckbadger-store`
- `cargo test -p ckbadger-indexer`
- `cargo test -p ckbadger-api`

## Non-Goals

- No attempt to remove `addr_txs` as a separate index in this change.
- No attempt to introduce a global event ID abstraction.
- No compatibility migration or partial backfill for existing RocksDB data.

## Result

- `activities`, `addr_txs`, and `nft_collection_activities` become true append-only history indexes.
- Orphaned history is preserved instead of deleted.
- Canonical views continue to be served correctly through domain-backed filtering.
- The store boundary becomes consistent with the approved design:
  immutable history in append-only, mutable canonical projections in domain.
