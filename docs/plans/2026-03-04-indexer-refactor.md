# Indexer Refactor Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Split the 15,888-line `sync/indexer.rs` monolith into focused modules, introduce `SyncMode` abstraction, consolidate writer modules, and decouple parser-writer data flow.

**Architecture:** Extract functions and types from `indexer.rs` into ~14 focused modules under `sync/`. Many extracted functions are free functions (not `impl Indexer` methods), so they move cleanly. For `impl Indexer` methods (batch, pipeline, reorg, sequential), Rust allows splitting impl blocks across files within the same crate. Writer consolidation merges `core.rs` into `mod.rs` and `blocks.rs`+`transactions.rs` into `chain.rs`.

**Tech Stack:** Rust (same crate, no new dependencies)

**Key Constraint:** Pure refactor — no behavioral changes. All 127 existing tests plus 12 integration test files must pass after each task. The verification command is `cargo test -p ckbadger-indexer`.

**Crate root:** `crates/indexer/`
**Target file:** `crates/indexer/src/sync/indexer.rs` (15,888 lines → ~3,500 lines)
**Module declaration:** `crates/indexer/src/sync/mod.rs`

---

## Phase 1: Foundation Modules (no Indexer impl, just free functions and types)

### Task 1: Extract `sync/types.rs` — shared bridge types

All struct and enum definitions that are consumed by multiple modules. This must come first since every subsequent module imports from here.

**Files:**

- Create: `crates/indexer/src/sync/types.rs`
- Modify: `crates/indexer/src/sync/mod.rs` (add module declaration)
- Modify: `crates/indexer/src/sync/indexer.rs` (replace definitions with `use`)

**What moves (structs & enums):**

- `PreParsedNftData` (line ~101)
- `DotbitConsumptionEvent` (line ~111)
- `DotbitTxActivityData` (line ~120)
- `XudtExtensionScript` (line ~130)
- `UndoSeqScope` enum (line ~442) + constants `UNDO_SEQ_SCOPE_SHIFT`, `UNDO_SEQ_LOCAL_MAX` (lines ~437-438)
- `SyncAction` enum (line ~1974)
- `ReorgAction` enum (line ~1982)
- `CachedCellInfo` (line ~2026) + related structs if any (`CachedUdtCellInfo`)
- `TxData` (line ~3202)
- `BatchWriteMetrics` (line ~1009)
- `UnresolvedLocalProbeSummary` + impl (lines ~784-880)
- `UnresolvedRpcProbeSummary` + impl (lines ~887-1009)

**Do NOT move yet:** `AdaptiveBatch*` structs (go to `adaptive.rs` in Phase 3), `IncidentReport` / `PerfStats` / `PipelinePerfStats` (go to `diagnostics.rs` in Phase 3), `RepeatedWarning*` (go to `diagnostics.rs` in Phase 3).

**Steps:**

1. Create `types.rs` with all listed structs/enums, preserving original attributes and derives
2. Add necessary `use` imports at top of `types.rs` (copy from `indexer.rs` header — `chrono`, `serde`, `ckbadger_store::types`, `std::collections`, etc.)
3. Make all moved items `pub(crate)` (or `pub(super)` if only used within `sync/`)
4. In `mod.rs`: add `mod types;` and `pub(crate) use types::*;` (or selective re-exports)
5. In `indexer.rs`: remove moved definitions, add `use super::types::*;` (or selective imports)
6. Run: `cargo check -p ckbadger-indexer` — fix any visibility or import errors
7. Run: `cargo test -p ckbadger-indexer` — all 127 tests + integration tests pass
8. Commit: `refactor(indexer): extract shared bridge types to sync/types.rs`

---

### Task 2: Extract `sync/sync_mode.rs` — bulk/live mode abstraction

Small, self-contained module. Introduces the `SyncMode` enum and migrates the 5 existing predicate functions.

**Files:**

- Create: `crates/indexer/src/sync/sync_mode.rs`
- Modify: `crates/indexer/src/sync/mod.rs`
- Modify: `crates/indexer/src/sync/indexer.rs`

**What moves (free functions):**

- `is_bulk_sync_active_by_lag` (line ~1033)
- `is_bulk_sync_batch` (line ~1037)
- `should_run_reorg_handling` (line ~1047)
- `should_skip_address_balances` (line ~1028)
- `ensure_bulk_sync_fresh_start` (line ~1051)

**New code to add:**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SyncMode {
    Bulk,
    Live,
}

impl SyncMode {
    pub fn from_lag(blocks_behind: u64, threshold: u64) -> Self {
        if blocks_behind > threshold { SyncMode::Bulk } else { SyncMode::Live }
    }
    pub fn is_bulk(&self) -> bool { matches!(self, SyncMode::Bulk) }
    pub fn should_handle_reorg(&self) -> bool { matches!(self, SyncMode::Live) }
    pub fn should_cache_proposals(&self) -> bool { matches!(self, SyncMode::Live) }
    pub fn should_invalidate_caches(&self) -> bool { matches!(self, SyncMode::Live) }
    pub fn should_accumulate_blocks(&self) -> bool { matches!(self, SyncMode::Live) }
    pub fn commit_with_wal(&self) -> bool { matches!(self, SyncMode::Live) }
    pub fn should_use_parallel_writes(&self) -> bool { matches!(self, SyncMode::Bulk) }
    pub fn fail_fast_on_error(&self) -> bool { matches!(self, SyncMode::Bulk) }
}
```

**Steps:**

1. Create `sync_mode.rs` with the `SyncMode` enum + methods above
2. Move the 5 existing predicate functions into the same file (they remain as free functions for now — replacing call sites with `SyncMode` methods happens in Phase 4 when we touch `batch.rs`/`pipeline.rs`)
3. Add `mod sync_mode;` to `mod.rs`
4. Update `indexer.rs` to `use super::sync_mode::*;`
5. Move the 8 related tests (`test_is_bulk_sync_*`, `test_should_run_reorg_*`, `test_ensure_bulk_sync_*`, `test_address_balances_are_never_skipped_*`) into `sync_mode.rs`'s `#[cfg(test)] mod tests`
6. Run: `cargo test -p ckbadger-indexer` — all tests pass
7. Commit: `refactor(indexer): extract SyncMode abstraction to sync/sync_mode.rs`

---

### Task 3: Extract `sync/helpers.rs` — utility functions

Pure utility functions with no domain knowledge. Used across multiple target modules.

**Files:**

- Create: `crates/indexer/src/sync/helpers.rs`
- Modify: `crates/indexer/src/sync/mod.rs`
- Modify: `crates/indexer/src/sync/indexer.rs`

**What moves (free functions):**

- `parse_prefixed_hex_u32` (line ~2185)
- `parse_prefixed_hex_u64` (line ~2193)
- `parse_outpoint_index_i16` (line ~2201)
- `checked_usize_to_i16` (line ~618)
- `checked_i32_to_i16` (line ~622)
- `checked_usize_to_i32` (line ~626)
- `tx_hash_key32` (line ~630)
- `short_tx_hash` (line ~775)
- `format_outpoint_sample` (line ~605)
- `blake160` (line ~1549)
- `panic_payload_to_string` (line ~1250)
- `duration_from_millis` (line ~1013)
- `atomic_saturating_sub_u64` (line ~1260)
- `encode_pipeline_reset_reason` (line ~141)
- `decode_pipeline_reset_reason` (line ~151)
- `encode_adaptive_batch_reason` (line ~161)
- `decode_adaptive_batch_reason` (line ~178)
- `decode_startup_phase` (line ~134)
- `parsed_input_outpoint_index_i16` (line ~340)
- Constants: `STARTUP_PHASE_NONE`, `STARTUP_PHASE_ROLLBACK_CLEANUP` (lines ~77-78)
- Constants: `PIPELINE_RESET_REASON_*` (lines ~79-83)
- Constants: `ADAPTIVE_REASON_*` (lines ~84-95)
- Constant: `PARTITION_SIZE` (line ~50)
- `get_partition_index` (line ~299)
- `format_partition_range` (line ~304)
- `crosses_partition_boundary` (line ~315)

**Steps:**

1. Create `helpers.rs`, move all listed functions + constants
2. All functions become `pub(crate)`
3. Add `mod helpers;` to `mod.rs`
4. Update `indexer.rs`: replace with `use super::helpers::*;`
5. Move related tests (~15 tests: type conversion, hex parsing, partition, pipeline reason roundtrip, startup phase decode) into `helpers.rs`'s `#[cfg(test)]`
6. Run: `cargo test -p ckbadger-indexer`
7. Commit: `refactor(indexer): extract utility functions to sync/helpers.rs`

---

## Phase 2: Domain Helper Modules (free functions with domain knowledge)

### Task 4: Extract `sync/undo.rs` — undo log and rollback helpers

**Files:**

- Create: `crates/indexer/src/sync/undo.rs`
- Modify: `crates/indexer/src/sync/mod.rs`
- Modify: `crates/indexer/src/sync/indexer.rs`

**What moves:**

- `next_undo_seq` (line ~449)
- `put_append_delete_undo_entry` (line ~469)
- `put_tx_context_undo_entries` (line ~487)
- `put_addr_tx_with_undo_log` (line ~544)
- `put_activity_with_undo_log` (line ~565)
- `rollback_undo_log_after_batch_cleanup` (line ~586)

Note: `UndoSeqScope` enum and `UNDO_SEQ_*` constants already moved to `types.rs` in Task 1.

**Steps:**

1. Create `undo.rs`, move functions, import `UndoSeqScope` from `super::types`
2. Add `mod undo;` to `mod.rs`
3. Update `indexer.rs`
4. Move undo-related tests (~6 tests) into `undo.rs`'s `#[cfg(test)]`
5. Run: `cargo test -p ckbadger-indexer`
6. Commit: `refactor(indexer): extract undo log helpers to sync/undo.rs`

---

### Task 5: Extract `sync/dao_helpers.rs` — DAO calculation functions

**Files:**

- Create: `crates/indexer/src/sync/dao_helpers.rs`
- Modify: `crates/indexer/src/sync/mod.rs`
- Modify: `crates/indexer/src/sync/indexer.rs`

**What moves (free functions):**

- `extract_dao_csu` (line ~2091)
- `split_secondary_issuance` (line ~2101)
- `resolve_non_miner_secondary_delta_for_snapshot` (line ~2158)
- `extract_ar_i64_from_dao` (line ~2293)
- `dao_csu_for_snapshot_date` (line ~2299)
- `derive_running_depositors` (line ~2313)
- `accumulate_dao_snapshot_deltas_for_txs` (line ~2344)
- `accumulate_secondary_issuance_deltas` (line ~2434)
- `checked_tx_fee` (line ~2252)
- `occupied_capacity_shannons_i128` (line ~1936)
- `occupied_capacity_shannons_i64` (line ~1952)
- `derive_pre_batch_live_cells` (line ~1917)

**Steps:**

1. Create `dao_helpers.rs`, move all functions
2. Import `TxData` from `super::types` if needed
3. Add `mod dao_helpers;` to `mod.rs`
4. Update `indexer.rs`
5. Move DAO-related tests (~22 tests) into `dao_helpers.rs`'s `#[cfg(test)]`
6. Run: `cargo test -p ckbadger-indexer`
7. Commit: `refactor(indexer): extract DAO calculation helpers to sync/dao_helpers.rs`

---

### Task 6: Extract `sync/nft_helpers.rs` — NFT classification and DotBit helpers

**Files:**

- Create: `crates/indexer/src/sync/nft_helpers.rs`
- Modify: `crates/indexer/src/sync/mod.rs`
- Modify: `crates/indexer/src/sync/indexer.rs`

**What moves:**

- `classify_nft_collection_id` (line ~1899)
- `DID_CKB_SENTINEL_COLLECTION` constant (line ~51)
- `dotbit_consume_event_order` (line ~2206)
- `dotbit_create_event_order` (line ~2221)
- `should_consume_dotbit_account` (line ~2232)
- `resolve_dotbit_account_id_for_outpoint` (line ~2239)
- `count_new_addresses` (line ~1879)

**Steps:**

1. Create `nft_helpers.rs`, move functions + constant
2. `DOTBIT_SENTINEL_COLLECTION` is imported from `crate::db::writer::dotbit` — keep that import
3. Add `mod nft_helpers;` to `mod.rs`
4. Update `indexer.rs`
5. Move NFT classification + DotBit tests (~8 tests) into `nft_helpers.rs`'s `#[cfg(test)]`
6. Run: `cargo test -p ckbadger-indexer`
7. Commit: `refactor(indexer): extract NFT/DotBit helpers to sync/nft_helpers.rs`

---

### Task 7: Extract `sync/token_helpers.rs` — XUDT, omnilock, and token parsing

This is the largest helper module due to molecule parsing functions.

**Files:**

- Create: `crates/indexer/src/sync/token_helpers.rs`
- Modify: `crates/indexer/src/sync/mod.rs`
- Modify: `crates/indexer/src/sync/indexer.rs`

**What moves:**

- All `XUDT_*` constants (lines ~67-73)
- `UNIQUE_TYPE_ARGS_LEN` (line ~74)
- `TOKEN_INFO_*` constants (lines ~75-76)
- All `OMNILOCK_*` constants (lines ~52-66)
- `OMNILOCK_CODE_HASHES: OnceLock` static (line ~97)
- `omnilock_code_hashes` (line ~1347)
- `is_omnilock_code_hash` (line ~1361)
- `extract_omnilock_supply_info_type_hash` (line ~1367)
- `parse_omnilock_supply_info_cell_data` (line ~1398)
- All `parse_molecule_*` functions (lines ~1422-1521)
- `extract_xudt_extension_scripts` (line ~1595)
- `extract_xudt_witness_extension_script_vec` (line ~1540)
- `extract_xudt_extension_scripts_from_witnesses` (line ~1561)
- `parse_xudt_extension_scripts_from_script_vec` (line ~1530)
- `parse_token_info_total_supply` (line ~1626)
- `collect_unique_cell_total_supply_by_type_args` (line ~1670)
- `observe_max_supply` (line ~1689)
- `collect_token_max_supply_observations` (line ~1713)
- `load_activity_token_info_cache` (line ~1806)

**Steps:**

1. Create `token_helpers.rs`, move all constants + functions
2. Import `XudtExtensionScript` from `super::types`
3. Add `mod token_helpers;` to `mod.rs`
4. Update `indexer.rs`
5. Move token/XUDT/omnilock tests (~13+ tests) into `token_helpers.rs`'s `#[cfg(test)]`
6. Move test helper functions that are only used by token tests (e.g., `dummy_xudt_cell`, `encode_xudt_witness`, `build_token_info_data`, etc.)
7. Run: `cargo test -p ckbadger-indexer`
8. Commit: `refactor(indexer): extract token/XUDT/omnilock helpers to sync/token_helpers.rs`

---

## Phase 3: Infrastructure Modules (structs with their own impl blocks)

### Task 8: Extract `sync/diagnostics.rs` — telemetry and performance tracking

**Files:**

- Create: `crates/indexer/src/sync/diagnostics.rs`
- Modify: `crates/indexer/src/sync/mod.rs`
- Modify: `crates/indexer/src/sync/indexer.rs`

**What moves:**

- `FLIGHT_RECORDER_CAPACITY` constant (line ~96)
- `IncidentReport` struct (line ~240)
- `PerfStats` struct + impl (lines ~2476-2556)
- `PipelinePerfStats` struct + impl (lines ~2558-2696)
- `RepeatedWarningSnapshot` struct (line ~1293)
- `RepeatedWarningState` struct (line ~1300)
- `RepeatedWarningTracker` struct + impl (lines ~1308-1346)
- Queue/memory helpers: `sender_queue_depth` (line ~1246), `queue_fill_percentage` (line ~1215), `parse_queue_capacity_txs` (line ~1222), `cgroup_memory_ratio_pct` (line ~1239), `should_trim_cell_cache` (line ~1276), `evict_committed_cell_cache_entries` (line ~1280)
- Pipeline idle helpers: `should_abort_pipeline_on_idle_timeout` (line ~1138), `should_invalidate_chart_caches_for_lag` (line ~1142), `should_log_unresolved_retry` (line ~771), `should_log_pipeline_idle_timeout` (line ~1211)

**Steps:**

1. Create `diagnostics.rs`, move all listed items
2. Note: `FlightRecorder` itself is already in `crate::runtime_diag` — just keep importing it
3. Add `mod diagnostics;` to `mod.rs`
4. Update `indexer.rs`
5. Move related tests (~13 tests: pipeline perf, queue %, repeated warning, memory, idle timeout)
6. Run: `cargo test -p ckbadger-indexer`
7. Commit: `refactor(indexer): extract diagnostics/telemetry to sync/diagnostics.rs`

---

### Task 9: Extract `sync/adaptive.rs` — AdaptiveBatchController

The entire adaptive batch sizing system with its ~50 constants.

**Files:**

- Create: `crates/indexer/src/sync/adaptive.rs`
- Modify: `crates/indexer/src/sync/mod.rs`
- Modify: `crates/indexer/src/sync/indexer.rs`

**What moves:**

- All `ADAPTIVE_BATCH_*` constants (lines ~2705-2745)
- `CELL_CACHE_CAPACITY`, `UDT_CELL_CACHE_CAPACITY` (lines ~2697-2698)
- `PARSER_UNRESOLVED_*` constants (lines ~2700-2703)
- `BULK_PHASE_COMMIT_SLOW_WARN_MS` (line ~2704)
- `AdaptiveBatchController` struct + impl (lines ~2805-3199)
- `plan_fetch_sub_batches` (line ~199)
- `adaptive_sub_batch_tx_cap` (line ~231)

Note: `AdaptiveBatchSnapshot`, `AdaptiveBatchProgressSnapshot`, `AdaptiveBatchInput`, `AdaptiveBatchAdjustment` already moved to `types.rs` in Task 1.

**Steps:**

1. Create `adaptive.rs`, move controller + constants + sub-batch planning
2. Import snapshot types from `super::types`
3. Add `mod adaptive;` to `mod.rs`
4. Update `indexer.rs`
5. Move adaptive batch tests (~22 tests) into `adaptive.rs`'s `#[cfg(test)]`
6. Run: `cargo test -p ckbadger-indexer`
7. Commit: `refactor(indexer): extract AdaptiveBatchController to sync/adaptive.rs`

---

## Phase 4: Indexer Impl Splits (methods that are `impl Indexer`)

These modules contain `impl Indexer { ... }` blocks in separate files. This is standard Rust — you can split impl blocks across files as long as the struct is defined in the same crate.

### Task 10: Extract `sync/reorg.rs` — reorg detection and fork handling

**Files:**

- Create: `crates/indexer/src/sync/reorg.rs`
- Modify: `crates/indexer/src/sync/mod.rs`
- Modify: `crates/indexer/src/sync/indexer.rs`

**What moves (impl Indexer methods):**

- `check_and_handle_reorg` (line ~12349)
- `find_fork_point` (line ~12515)
- `run_proposal_cache_batch` (line ~12554)
- `get_chain_block_hash` (line ~12318)
- `get_chain_tip` (line ~12336)
- `reconcile_hodl_tracker_with_tip` (line ~12222)
- `update_hodl_wave` (line ~12232)

**Structure of new file:**

```rust
use super::indexer::Indexer;
// ... other imports

impl Indexer {
    pub(crate) async fn check_and_handle_reorg(&self, ...) -> Result<ReorgAction> { ... }
    pub(crate) async fn find_fork_point(&self, ...) -> Result<u64> { ... }
    // etc.
}
```

**Steps:**

1. Create `reorg.rs` with `impl Indexer` block
2. Move the 7 methods listed above
3. Methods that were `async fn` (private) become `pub(super) async fn` or `pub(crate) async fn`
4. Add `mod reorg;` to `mod.rs` (NO re-export needed — methods are on `Indexer`)
5. Update `indexer.rs`: remove moved methods from its `impl Indexer` block
6. Move reorg-related tests if any exist in the test section
7. Run: `cargo test -p ckbadger-indexer`
8. Commit: `refactor(indexer): extract reorg handling to sync/reorg.rs`

---

### Task 11: Extract `sync/batch.rs` — batch sync orchestration

The largest extraction. Contains the main batch processing logic including `write_parsed_batch` (~3500 lines).

**Files:**

- Create: `crates/indexer/src/sync/batch.rs`
- Modify: `crates/indexer/src/sync/mod.rs`
- Modify: `crates/indexer/src/sync/indexer.rs`

**What moves (impl Indexer methods):**

- `sync_batch` (line ~6568)
- `write_parsed_batch` (line ~8751)
- `sync_blocks_batch` (line ~6887) — the rayon parallel parser
- `cleanup_failed_batch_range` (line ~6766)
- `maybe_start_label_import` (line ~6814)
- `maybe_invalidate_chart_caches` (line ~6552)
- `check_bulk_sync_completion` (line ~6717)
- `write_batch_stats_to_batch` (line ~12000)

**Also moves (free functions used only by batch):**

- `collect_missing_input_outpoints` (line ~319)
- `build_activity_input_views` (line ~355)
- `parse_parsed_cell_udt_amount` (line ~2053)
- `parse_udt_cells_with_store_fallback_inner` (line ~640) (if it's only used in batch context)
- `resolve_input_udt_info_from_live_cells` (line ~712)
- `next_fetch_start_after_batch` (line ~1018)

**Steps:**

1. Create `batch.rs` with `impl Indexer` block + free helper functions
2. Import from `super::types`, `super::helpers`, `super::undo`, `super::dao_helpers`, `super::nft_helpers`, `super::token_helpers`, `super::sync_mode`, `super::diagnostics`
3. This is the module where `SyncMode` methods start getting used — replace inline `blocks_behind > threshold` checks with `SyncMode` method calls where they appear in moved code
4. Add `mod batch;` to `mod.rs`
5. Update `indexer.rs`
6. Move batch-related tests (activity input views, missing outpoints, UDT parsing, etc.) into `batch.rs`'s `#[cfg(test)]`
7. Run: `cargo test -p ckbadger-indexer`
8. Commit: `refactor(indexer): extract batch sync orchestration to sync/batch.rs`

---

### Task 12: Extract `sync/pipeline.rs` — three-stage async pipeline

**Files:**

- Create: `crates/indexer/src/sync/pipeline.rs`
- Modify: `crates/indexer/src/sync/mod.rs`
- Modify: `crates/indexer/src/sync/indexer.rs`

**What moves (impl Indexer methods):**

- `run_pipeline` (line ~4202)
- `fetch_blocks_with_config` (line ~6494)
- `fetch_blocks_direct` (line ~6522)
- `fetch_blocks_parallel` (line ~6862)
- `drain_channel` (line ~6544)

**Steps:**

1. Create `pipeline.rs` with `impl Indexer` block
2. `run_pipeline` calls `sync_batch` (now in `batch.rs`) and `check_and_handle_reorg` (now in `reorg.rs`) — these work because they're all `impl Indexer` methods
3. Add `mod pipeline;` to `mod.rs`
4. Update `indexer.rs`
5. Run: `cargo test -p ckbadger-indexer`
6. Commit: `refactor(indexer): extract pipeline stages to sync/pipeline.rs`

---

### Task 13: Extract `sync/sequential.rs` — sequential sync mode

**Files:**

- Create: `crates/indexer/src/sync/sequential.rs`
- Modify: `crates/indexer/src/sync/mod.rs`
- Modify: `crates/indexer/src/sync/indexer.rs`

**What moves (impl Indexer method):**

- `run_sequential` (line ~4136)

**Steps:**

1. Create `sequential.rs` with `impl Indexer` block containing `run_sequential`
2. Add `mod sequential;` to `mod.rs`
3. Update `indexer.rs`
4. Run: `cargo test -p ckbadger-indexer`
5. Commit: `refactor(indexer): extract sequential sync to sync/sequential.rs`

---

### Task 14: Clean up remaining `sync/indexer.rs`

After Tasks 1-13, `indexer.rs` should contain only:

- `Indexer` struct definition (~30 fields)
- `Indexer::new()` constructor
- `Indexer::run()` dispatcher (calls `run_sequential`/`run_pipeline`)
- Public accessor methods (`progress()`, `cache_invalidator()`, `writer()`, `is_bulk_sync_active()`, etc.)
- `ensure_compaction_mode()`
- Flight recorder/incident methods (`record_flight_event()`, `report_incident()`, `write_incident_report()`, etc.)
- Any remaining helper methods that access `&self` and didn't fit elsewhere

**Target:** ~800-1500 lines (down from 15,888).

**Steps:**

1. Review what remains in `indexer.rs` — identify any orphaned functions
2. Move any remaining test helpers (dummy builders: `dummy_live_cell_info`, `dummy_cached_cell_info`, `molecule_*` test helpers) to a `#[cfg(test)]` helper module or to the test modules that use them
3. Verify the `#[cfg(test)] mod tests` block in `indexer.rs` only contains tests for code that remains in `indexer.rs`
4. Run: `cargo test -p ckbadger-indexer`
5. Run: `cargo clippy -p ckbadger-indexer` — fix any warnings about unused imports
6. Commit: `refactor(indexer): clean up indexer.rs after module extraction`

---

## Phase 5: Writer Consolidation

### Task 15: Merge `writer/core.rs` into `writer/mod.rs`

`core.rs` is only 46 lines containing the `BatchWriter` struct. It's the root type of the writer module.

**Files:**

- Delete: `crates/indexer/src/db/writer/core.rs`
- Modify: `crates/indexer/src/db/writer.rs` (the module declaration file)

**Current `writer.rs` (module file):**

```rust
mod core;
// ... other mods
pub use core::BatchWriter;
```

**Steps:**

1. Read `core.rs` — it contains `BatchWriter` struct + `new()`, `with_fast_sync_mode()`, `with_cache()`, `cache_invalidator()`, `store()` methods
2. Move the entire content of `core.rs` into the top of `writer.rs` (above module declarations)
3. Remove `mod core;` and `pub use core::BatchWriter;` — `BatchWriter` is now defined directly in the module file
4. Delete `core.rs`
5. Run: `cargo test -p ckbadger-indexer`
6. Commit: `refactor(indexer): merge writer/core.rs into writer module root`

---

### Task 16: Merge `writer/blocks.rs` + `writer/transactions.rs` into `writer/chain.rs`

Both are tiny (36 + 55 = 91 lines) and write chain-level data.

**Files:**

- Create: `crates/indexer/src/db/writer/chain.rs`
- Delete: `crates/indexer/src/db/writer/blocks.rs`
- Delete: `crates/indexer/src/db/writer/transactions.rs`
- Modify: `crates/indexer/src/db/writer.rs`

**Steps:**

1. Create `chain.rs` with a single `impl BatchWriter` block
2. Copy `insert_blocks_batch` from `blocks.rs` and `insert_transactions_batch` from `transactions.rs`
3. Combine imports from both files
4. In `writer.rs`: replace `mod blocks; mod transactions;` with `mod chain;`
5. Delete `blocks.rs` and `transactions.rs`
6. Run: `cargo test -p ckbadger-indexer`
7. Commit: `refactor(indexer): merge blocks + transactions writers into writer/chain.rs`

---

## Phase 6: Parser-Writer Decoupling

### Task 17: Refactor `insert_transactions_batch` to accept `&[&TxData]`

Currently accepts a 17-element tuple. Change to accept `&TxData` directly.

**Files:**

- Modify: `crates/indexer/src/db/writer/chain.rs` (created in Task 16)
- Modify: `crates/indexer/src/sync/batch.rs` (created in Task 11, contains call site)

**Before (in chain.rs):**

```rust
pub fn insert_transactions_batch(
    &self,
    txs: &[(&[u8], i64, &[u8], i32, i32, i16, i16, i16, i16, i16, i64, i64, i64, Option<i32>, Option<i64>, bool, DateTime<Utc>)],
    batch: &mut StoreBatch,
) -> Result<()>
```

**After (in chain.rs):**

```rust
pub fn insert_transactions_batch(
    &self,
    txs: &[&TxData],
    batch: &mut StoreBatch,
) -> Result<()> {
    for tx in txs {
        let entry = TxIndexEntry {
            block_number: tx.block_number,
            block_hash: tx.block_hash.clone(),
            tx_index: tx.tx_index,
            // ... unpack fields from TxData
        };
        batch.put_tx_index(&tx.hash, &entry)?;
        batch.put_tx_hash_map(tx.block_number, tx.tx_index, &tx.hash)?;
    }
    Ok(())
}
```

**Steps:**

1. Update `chain.rs`: change signature to `&[&TxData]`, unpack `TxData` fields inside
2. Update call site in `batch.rs`: remove the tuple construction, pass `&tx_data` references directly
3. Run: `cargo test -p ckbadger-indexer`
4. Commit: `refactor(indexer): writer accepts TxData directly instead of positional tuples`

---

### Task 18: Final verification and cleanup

**Steps:**

1. Run full test suite: `cargo test -p ckbadger-indexer`
2. Run clippy: `cargo clippy -p ckbadger-indexer`
3. Run full workspace check: `cargo check`
4. Run integration tests: `cargo test -p ckbadger-indexer -- --test` (if separate)
5. Verify no `#[allow(unused)]` was added to suppress extraction artifacts
6. Count lines in `indexer.rs` — should be ~800-1500
7. Verify module structure matches design:
   ```
   sync/
     mod.rs, indexer.rs, types.rs, sync_mode.rs, pipeline.rs,
     sequential.rs, batch.rs, reorg.rs, adaptive.rs,
     diagnostics.rs, helpers.rs, dao_helpers.rs, nft_helpers.rs,
     token_helpers.rs, undo.rs, progress.rs
   db/writer/
     mod.rs (with BatchWriter), chain.rs, sync.rs, cells.rs,
     addresses.rs, activities.rs, statistics.rs, dao.rs,
     udt.rs, spore.rs, mnft.rs, dotbit.rs, reorg.rs,
     hodl_wave.rs, nft_activity_acc.rs
   ```
8. Commit: `refactor(indexer): final cleanup after indexer refactor`

---

## Execution Order & Dependencies

```
Phase 1 (Foundation):
  Task 1 (types.rs)
    → Task 2 (sync_mode.rs)     [imports from types]
    → Task 3 (helpers.rs)        [no type deps]

Phase 2 (Domain helpers, all depend on Task 1):
  Task 4 (undo.rs)
  Task 5 (dao_helpers.rs)
  Task 6 (nft_helpers.rs)
  Task 7 (token_helpers.rs)

Phase 3 (Infrastructure, depend on Tasks 1+3):
  Task 8 (diagnostics.rs)
  Task 9 (adaptive.rs)

Phase 4 (Indexer impl splits, depend on all above):
  Task 10 (reorg.rs)
  Task 11 (batch.rs)        ← largest, most dependencies
  Task 12 (pipeline.rs)     ← depends on batch.rs existing
  Task 13 (sequential.rs)
  Task 14 (cleanup)

Phase 5 (Writer, independent of Phase 1-4):
  Task 15 (core→mod)
  Task 16 (blocks+txs→chain)

Phase 6 (Decoupling, depends on Tasks 11+16):
  Task 17 (TxData in writer signatures)
  Task 18 (final verification)
```

Tasks within a phase can be parallelized where noted. Tasks 15-16 (writer consolidation) can run in parallel with Phase 1-4.

## Risk Mitigation

- **Visibility errors**: Most common issue. When moving a private `fn` to a new file, it needs `pub(crate)` or `pub(super)`. Fix during `cargo check` after each move.
- **Circular imports**: Avoided by the module hierarchy — helpers don't import from batch/pipeline/reorg.
- **Test relocation**: Some tests reference private functions. If a test tests a now-`pub(crate)` function, it can stay. If it tests module internals, move it with the code.
- **Large diffs**: Each task is one commit. If a task's diff is too large to review, split it (e.g., Task 11 could be split into "move sync_batch" + "move write_parsed_batch").
