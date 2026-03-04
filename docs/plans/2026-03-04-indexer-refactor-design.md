# Indexer Refactor Design

Date: 2026-03-04

## Goal

Break the 15,888-line `sync/indexer.rs` monolith into focused modules, introduce a `SyncMode` abstraction for bulk/live branching, consolidate writer modules, and decouple parser-writer data flow.

## Principle Alignment

- **CKB Native**: No domain logic changes; refactor only
- **Local First**: Write-path performance preserved (parallel bulk writes, WAL-free commits unchanged)
- **Agent Friendly**: Smaller modules with clear ownership improve navigability

## Current State

- `sync/indexer.rs`: 15,888 lines, 365 functions, 17 structs, 3 enums
- 52 bulk/live sync conditionals scattered throughout
- `TxData` and `PreParsedNftData` bridge types defined inside indexer.rs
- Writer modules: 17 files ranging from 36 to 2,315 lines
- Writer methods accept destructured tuples instead of typed structs

## Design

### 1. Module Extraction

Split `sync/indexer.rs` into focused modules:

```
sync/
  mod.rs                 # re-exports
  indexer.rs             # Indexer struct, new(), run(), public API (~800 lines)
  types.rs               # TxData, PreParsedNftData, CachedCellInfo, enums (~400 lines)
  sync_mode.rs           # SyncMode enum + all bulk/live behavior methods (~200 lines)
  pipeline.rs            # run_pipeline() - 3-stage async pipeline (~2500 lines)
  sequential.rs          # run_sequential() - non-pipelined path (~800 lines)
  batch.rs               # sync_batch(), write_parsed_batch() (~3500 lines)
  reorg.rs               # check_and_handle_reorg(), find_fork_point() (~2200 lines)
  adaptive.rs            # AdaptiveBatchController + constants (~1500 lines)
  diagnostics.rs         # FlightRecorder, IncidentReport, PerfStats, PipelinePerfStats (~800 lines)
  helpers.rs             # Hex parsing, molecule encoding, blake160, type conversions (~600 lines)
  dao_helpers.rs         # DAO snapshot deltas, issuance splitting, CSU extraction (~400 lines)
  nft_helpers.rs         # NFT collection ID, DotBit events, PreParsedNftData building (~500 lines)
  token_helpers.rs       # XUDT extension scripts, omnilock parsing, max supply (~500 lines)
  undo.rs                # UndoSeqScope, undo log helpers, rollback (~200 lines)
```

Tests move from the end of indexer.rs to co-located `#[cfg(test)]` blocks in each module.

### 2. SyncMode Abstraction

Replace 52 scattered bulk/live conditionals with a single enum:

```rust
pub enum SyncMode {
    Bulk,
    Live,
}

impl SyncMode {
    pub fn from_lag(blocks_behind: u64, threshold: u64) -> Self;
    pub fn is_bulk(&self) -> bool;
    pub fn should_handle_reorg(&self) -> bool;       // Live only
    pub fn should_cache_proposals(&self) -> bool;     // Live only
    pub fn should_invalidate_caches(&self) -> bool;   // Live only
    pub fn should_accumulate_blocks(&self) -> bool;   // Live only (2s wait)
    pub fn commit_with_wal(&self) -> bool;            // Live=WAL, Bulk=no-WAL
    pub fn should_use_parallel_writes(&self) -> bool; // Bulk only (rayon)
    pub fn fail_fast_on_error(&self) -> bool;         // Bulk only
}
```

Computed once per batch in `sync_batch()`, threaded through to callees.

### 3. Writer Module Consolidation

17 modules to 15:

- `core.rs` (46 lines) merges into `mod.rs` (BatchWriter struct is root type)
- `blocks.rs` (36 lines) + `transactions.rs` (55 lines) merge into `chain.rs`
- `dao.rs` (2,315 lines) stays intact (cohesive state machine)
- All other modules unchanged

### 4. Parser-Writer Decoupling

**Bridge types move to `sync/types.rs`**: TxData, PreParsedNftData, CachedCellInfo, CachedUdtCellInfo, DotbitConsumptionEvent, DotbitTxActivityData, XudtExtensionScript, SyncAction, ReorgAction, UndoSeqScope.

**Writer methods accept `&TxData` directly** instead of positional tuples. Each writer unpacks what it needs internally.

No parser module changes needed — parsers already produce clean types.

## Constraints

- Pure refactor: no behavioral changes, no new features
- All existing tests must pass without modification (only file relocation)
- No storage/schema impact
- Re-sync NOT required
