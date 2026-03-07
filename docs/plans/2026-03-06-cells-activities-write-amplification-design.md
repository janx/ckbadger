# Cells and Activities Write Amplification Reduction Design

## Goal

- Reduce bulk-sync write amplification by restructuring the `cells` and `activities` storage families instead of relying on RocksDB tuning.
- Preserve exact inline index construction during bulk sync and live sync.
- Accept more complex read paths in exchange for materially cheaper write paths.

## Principle Alignment

- CKB Native: the design keeps canonical chain state in one mutable path and treats activity/history as derived append-only records.
- Local First: the design optimizes rebuild throughput and accepts bounded unreachable history garbage after reorgs instead of paying delete costs on every hot-path write.
- Agent Friendly: each visible read result is derived from one canonical mutable state (`cell_state`, canonical tx location) plus append-only historical structures with explicit filtering rules.

## Problem Summary

- Current `cells` writes pay repeatedly for:
  - canonical payload persistence
  - live marker writes/deletes
  - consumed metadata writes
  - 4 separate secondary-index put/delete paths
- Current `activities` writes pay repeatedly for:
  - one full `ActivityEntry` per owner
  - repeated `tx_hash`, `timestamp`, `block_number`, `tx_index`, `is_cellbase`
  - repeated `peers`, which grows roughly quadratically with participant count
- RocksDB configuration can smooth the symptoms but cannot remove these structural costs.

## Constraints

- `docs/prompts/BULK_SYNC.md` remains authoritative:
  - all required data must be written inline on the canonical block path
  - no delayed/on-demand index construction
  - bulk sync stays single-shot rebuild only
- Read paths may become more complex, but externally visible results must remain exact.
- Schema-breaking changes and full re-sync are acceptable.
- Append-only store semantics must remain true append-only:
  - no update
  - no delete
  - no overwrite

## Approaches Considered

### Approach 1: Collapse only mutable cell state

- Merge `cells`, `live_cells`, and `consumed_cells` into a single mutable record.
- Leave 4 cell indexes and `CF_ACTIVITIES` mostly intact.

Trade-offs:

- Low-risk change to current readers.
- Still pays consume-time secondary-index deletes.
- Does not address activity payload duplication.

### Approach 2: Append-only indexes plus normalized activity envelopes

- Split immutable cell payload from mutable cell state.
- Make cell secondary indexes append-only historical indexes.
- Replace per-owner full activity rows with tx-level envelopes plus owner references.

Trade-offs:

- Largest structural reduction in hot-path writes.
- Read path becomes a join/filter flow.
- Requires broader store, writer, and API changes.

### Approach 3: Generic unified relation store

- Collapse cells and activities into a small number of generic tagged relation CFs.

Trade-offs:

- Architecturally tidy on paper.
- High risk of mixing very different access/compaction patterns.
- Adds abstraction without clearly beating Approach 2 on throughput.

## Recommendation

- Use Approach 2.
- It attacks the two dominant write-amplification sources directly:
  - consume-time cell index churn
  - repeated activity payload bytes
- It also matches the project’s write-first worldview better than parameter tuning or generic abstraction.

## Proposed Design

### 1. Store Boundary

Domain store keeps only mutable canonical/query state:

- `cell_state`
- `tx_hash_map`
- `addr_balance`
- `reorg_undo_log_by_block`
- existing mutable canonical aggregates/statistics

Append-only store gains immutable history/index structures:

- `cell_payloads`
- `cell_index`
- `activity_tx_envelopes`
- `activity_by_owner`

This intentionally removes `activities` from the mutable domain store and aligns it with append-only history semantics.

### 2. Cells Schema

#### `cell_state` (domain)

- Key: `outpoint`
- Value:
  - `created_at_block`
  - `payload_key`
  - `status` (`live` or `consumed`)
  - `consumed_at_block`
  - `consumed_by_tx`

This is the only canonical mutable cell state.

#### `cell_payloads` (append-only)

- Key: `created_at_block + outpoint`
- Value: immutable cell payload

`created_at_block` is included so the same transaction hash/output can be re-included on a different fork without append-only key collision.

#### `cell_index` (append-only)

- Key: `index_tag + hash_or_code_hash + created_at_block + outpoint`
- Value: empty

`index_tag` logical namespaces:

- `lock`
- `type`
- `lock_code`
- `type_code`

This replaces 4 separate CFs with one append-only index CF.

### 3. Cells Write Path

Cell creation:

1. append `cell_payloads`
2. upsert `cell_state` as live
3. append 2 to 4 `cell_index` entries

Cell consumption:

1. update `cell_state` to consumed

Notably absent on consume:

- no payload rewrite
- no live marker delete
- no consumed-meta side CF write
- no index delete

### 4. Cells Read Path

`get_cell(outpoint)`:

1. read `cell_state`
2. require `status == live`
3. follow `payload_key`
4. read `cell_payloads`

`get_consumed_cell_info(outpoint)`:

1. read `cell_state`
2. require `status == consumed`
3. follow `payload_key`
4. read `cell_payloads`

`list_cells_by_*`:

1. prefix-scan `cell_index`
2. decode `created_at_block + outpoint`
3. batch-read `cell_state`
4. keep only rows where:

- `cell_state.status == live`
- `cell_state.created_at_block == index.created_at_block`

5. batch-read payloads for survivors

This preserves exact results while allowing stale historical index entries to remain in storage.

### 5. Activities Schema

#### `activity_tx_envelopes` (append-only)

- Key: `block_desc + tx_idx + tx_hash`
- Value:
  - `tx_hash`
  - `block_number`
  - `tx_index`
  - `timestamp`
  - `is_cellbase`
  - `participants: Vec<lock_hash>`
  - `owner_views: Vec<OwnerActivityViewStored>`

`OwnerActivityViewStored` contains only owner-local information:

- `ckb_delta`
- `occupied_delta`
- `asset_changes`

It does **not** store `peers`.

#### `activity_by_owner` (append-only)

- Key: `lock_hash + block_desc + tx_idx + tx_hash`
- Value: `owner_slot`

This is the read-optimized owner-to-envelope reference.

### 6. Activities Write Path

For each transaction:

1. build owner accumulators once
2. assign deterministic owner slots
3. write one `activity_tx_envelopes` row
4. write one `activity_by_owner` row per owner

This removes repeated storage of:

- `tx_hash`
- `block_number`
- `tx_index`
- `timestamp`
- `is_cellbase`
- full `peers` vectors per owner

### 7. Activities Read Path

Address activities:

1. prefix-scan `activity_by_owner`
2. read `owner_slot`
3. batch-get `activity_tx_envelopes`
4. canonical-filter via `tx_hash_map(tx_hash) == (block_num, tx_idx)`
5. project the owner-local view from `owner_slot`
6. reconstruct `peers` as `participants - self`
7. apply activity filter (`ckb`, `token`, `nft`, `dao`)

The read path becomes more join-heavy, but the write path becomes much lighter.

### 8. Reorg and Correctness Rules

`cell_state` is the only canonical mutable state for cells.

Append-only rows may remain after reorg:

- orphaned `cell_payloads`
- orphaned `cell_index`
- orphaned `activity_tx_envelopes`
- orphaned `activity_by_owner`

They must be invisible because readers apply canonical filters:

- cells: `state.created_at_block == index.created_at_block` and `state.status == live`
- activities: `tx_hash_map(tx_hash) == (block_num, tx_idx)`

Undo-log responsibility:

- rollback only mutable domain state
- do not delete append-only rows during reorg

### 9. Invariants

- `get_cell(outpoint)` returns only when `cell_state.status == live`.
- `get_consumed_cell_info(outpoint)` returns only when `cell_state.status == consumed`.
- A cell index hit is visible only when the index creation block matches the current canonical `cell_state.created_at_block`.
- An activity row is visible only when `tx_hash_map(tx_hash)` points to the same `(block_num, tx_idx)` carried by the append-only record.
- Any second write to the same append-only key is an invariant violation and must fail fast.

## Expected Impact

### Cells

- Create path remains similar in total key count, but CF fanout is reduced.
- Consume path becomes dramatically cheaper because it mutates only `cell_state`.
- Secondary index compaction should shift from delete-heavy churn to append-heavy history.

### Activities

- Shared metadata is written once per tx instead of once per owner.
- `peers` storage drops from per-owner duplication to read-time reconstruction.
- Large multi-owner transactions should shrink sharply in bytes written.

## Risks

- Cursor/page stability when dead index entries accumulate.
- Canonical filtering bugs that accidentally surface orphaned append-only history.
- Reorg edge cases where the same tx hash reappears on a different fork.
- Over-tuning RocksDB before schema changes land.

## Validation Strategy

- Cell lifecycle tests:
  - create -> visible in all cell readers
  - consume -> removed from live readers, visible from consumed readers
  - rollback consume -> visible live again
  - stale orphaned index/payload present -> reader still returns only canonical live cells
- Activity tests:
  - single-owner tx
  - multi-owner tx
  - high-peer tx
  - stale orphaned owner refs/envelopes present -> reader still returns only canonical txs
- Bulk-sync perf validation:
  - compare `avg_commit_ms`
  - compare `p99_commit_ms`
  - compare `t1_ms`
  - compare `t_act_ms`
  - compare cumulative compaction write GB for cell/activity CFs

## Affected Areas

- `crates/ckbadger-store/src/store.rs`
- `crates/ckbadger-store/src/keys.rs`
- `crates/ckbadger-store/src/types.rs`
- `crates/ckbadger-store/src/batch.rs`
- `crates/ckbadger-store/src/cell_ops.rs`
- `crates/ckbadger-store/src/activity_ops.rs`
- `crates/ckbadger-store/src/reorg_ops.rs`
- `crates/indexer/src/sync/batch.rs`
- `crates/indexer/src/sync/undo.rs`
- `crates/indexer/src/db/writer/activities.rs`
- `crates/api/src/routes/activities.rs`
- `crates/api/src/routes/cells.rs`
