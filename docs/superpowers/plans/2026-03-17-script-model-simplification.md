# Script Model Simplification Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate script reference/version indexer-time computation, restore bulk sync from 3688 to ≥7000 blk/s.

**Architecture:** Remove all script version pre-computation from the indexer pipeline (parser dep cell resolution, per-cell version writes, reference/version stats). Delete CF_CELL_SCRIPT_VERSIONS and CF_SCRIPT_REFERENCES. Rewrite API script resolution to derive hash_type and version from existing cell indexes + CF_SCRIPT_INFO at query time.

**Tech Stack:** Rust, RocksDB (ckbadger-store), Axum (API), Vite+React (frontend)

**Spec:** `docs/plans/2026-03-17-script-model-simplification-design.md`

---

## Chunk 1: Indexer Deletion

### Task 1: Remove script version computation from indexer (parser + writer)

Remove `build_script_reference_version_state()` and all supporting code from the parser thread, AND remove the writer-side consumers (`put_cell_script_version`, `update_script_reference_version_batch`). These must be in the same task because they share `ParsedBatch` fields — removing fields without removing usage won't compile.

**Files:**
- Modify: `crates/indexer/src/sync/pipeline.rs`
- Modify: `crates/indexer/src/sync/batch.rs`
- Modify: `crates/indexer/src/sync/types.rs` (if `cell_deps` field on `TxData`)
- Modify: `crates/indexer/src/db/writer/addresses.rs`
- Modify: `crates/indexer/src/bulk_sync_perf.rs`

- [ ] **Step 1: Remove ParsedBatch fields and parser cache from pipeline.rs**

Delete:
- `ParserVersionCache` type alias (line 77)
- `reset_parser_version_cache_epoch()` function (lines 85-98)
- `prune_parser_version_cache()` function (lines 99-112)
- All `parser_version_cache` variable declarations and usage throughout the file
- `script_version_ms` field from `ParserBatchPerfSample` (line 74) and `ParserPrecomputePhaseMetrics` (line 55)
- `script_version_ms` from `ParserPrecomputePhaseMetrics::total_ms()` sum (line 65)
- The script version computation block (~lines 1939-1968) that calls `build_script_reference_version_state()`
- Cache population loop after script version computation (~lines 1975-1981)
- `cell_script_version_rows` and `script_reference_version_changes` from `ParsedBatch` struct fields AND all send/receive sites (lines 444-445, 2094-2095, 2186-2187, 2405-2406)
- `script_version_ms` from perf sample recording (line 2069, 2667)
- Pipeline tests: `test_parser_cache_committed_tip_from_sync_tip_retains_genesis_cells`, `test_parser_cache_epoch_reset_clears_old_entries_and_keeps_replayed_blocks`, `sample_cell_script_version_info` helper

- [ ] **Step 2: Remove script version functions from batch.rs**

Delete:
- `ScriptReferenceVersionChanges` type alias (lines 818-819)
- `add_script_reference_version_delta()` function (~lines 869-900)
- `DepCellCache` struct and impl (~lines 891-909)
- `load_dep_cell_info()` function (~lines 910-970)
- `load_dep_cell_data()` function (~lines 1069-1112)
- `resolve_tx_dep_cells()` function (~lines 1113-1162)
- `build_script_reference_version_state()` function (~lines 1246+, ~400 lines)
- `put_cell_script_version()` loops in T1 writer thread (lines 2569-2571, 3934-3935)
- `update_script_reference_version_batch()` calls in T2 writer thread (lines 2657-2661, 4011-4015)
- Tests: `test_resolve_tx_dep_cells_*`, `test_build_script_reference_version_state_*` (lines 6561-7001)

- [ ] **Step 3: Remove writer functions from addresses.rs**

Delete:
- `read_script_references()` (lines 614-658)
- `read_script_versions()` (lines 661-697)
- `apply_script_reference_version_deltas()` (lines 700-976)
- `update_script_reference_version_batch()` (lines 979-1025)
- Related tests (lines 1323-1477)

- [ ] **Step 4: Remove script_version_ms from bulk_sync_perf.rs**

Delete `script_version_ms` field from `BatchSample` struct (line 38) and its default (line 87).

- [ ] **Step 5: Remove TxData.cell_deps field if only used by deleted code**

Check `crates/indexer/src/sync/types.rs` line 127 — if `cell_deps` field on `TxData` is only consumed by `resolve_tx_dep_cells()` (now deleted), remove the field. Also remove the `TransactionParser::parse_cell_deps(tx)` call in the parser that populates it.

- [ ] **Step 6: Verify compilation**

Run: `cargo check -p ckbadger-indexer`
Expected: Compiles (warnings for unused store functions acceptable — cleaned in Task 4)

- [ ] **Step 7: Run indexer tests**

Run: `cargo test -p ckbadger-indexer --lib`
Expected: All remaining tests pass

- [ ] **Step 8: Commit**

```
git add crates/indexer/
git commit -m "refactor: remove script version computation from indexer

Remove build_script_reference_version_state(), dep cell resolution,
parser_version_cache, per-cell put_cell_script_version() writes in T1,
update_script_reference_version_batch() in T2, and all supporting code.
Eliminates 95.8M WriteBatch entries (~16.8GB) and parser dep cell
resolution that caused 48% bulk sync regression.

Part of script model simplification (docs/plans/2026-03-17)."
```

---

### Task 2: Remove script reference/version delta path from reorg

Remove the `accumulate_script_reference_version_deltas()` function and all calls to `get_cell_script_version()` in the rollback path. CF_SCRIPT_INFO reorg via `accumulate_cell_deltas()` / `apply_script_usage_deltas()` remains unchanged.

**Files:**
- Modify: `crates/ckbadger-store/src/reorg_ops.rs`

- [ ] **Step 1: Remove script reference/version delta types and functions**

Delete:
- `ScriptReferenceVersionDeltaKey` and `ScriptReferenceVersionDelta` type aliases (lines 12-13)
- `accumulate_script_reference_version_deltas()` function (lines 282-340+)

- [ ] **Step 2: Remove delta accumulation from rollback**

Delete:
- `script_reference_version_deltas` variable declaration (~line 966)
- All `get_cell_script_version()` calls (lines 1036, 1117, 1188, 1280)
- All `accumulate_script_reference_version_deltas()` calls (lines 1044-1047, 1125-1128, 1196-1199, 1288-1291)
- Script reference/version delta application loop (lines 1899-2057)
- Counter variables `script_references_updated`, `script_versions_updated` (lines 1762-1763) and their usage in log statements (lines 2161-2162, 2171-2172)
- `delete_cf(cf_cell_script_versions())` in `delete_cell_index_entries()` (line 184)

Keep: All `script_info_deltas` / `accumulate_cell_deltas` code (CF_SCRIPT_INFO rollback — independent and unchanged).

- [ ] **Step 3: Remove reorg tests for script reference/version**

Delete: `test_rollback_updates_script_reference_and_version_rows()` (lines 4366-4687)

- [ ] **Step 4: Verify compilation and run reorg tests**

Run: `cargo check -p ckbadger-store && cargo test -p ckbadger-store -- rollback`
Expected: Compiles. Remaining rollback tests pass.

- [ ] **Step 5: Commit**

```
git add crates/ckbadger-store/src/reorg_ops.rs
git commit -m "refactor: remove script reference/version delta path from reorg

Remove accumulate_script_reference_version_deltas() and all
get_cell_script_version() calls in rollback. CF_SCRIPT_INFO rollback
path (accumulate_cell_deltas/apply_script_usage_deltas) unchanged.

Part of script model simplification (docs/plans/2026-03-17)."
```

---

## Chunk 2: API Rewrite + Store Cleanup

### Task 3: Rewrite API script resolution to use cell indexes

Replace CF_SCRIPT_REFERENCES-based resolution with cell-index-based derivation. Must update ALL API consumers: scripts.rs, search.rs, cells.rs, warmup.rs, and utils/mod.rs re-exports.

**Files:**
- Modify: `crates/api/src/utils/script_resolution.rs`
- Modify: `crates/api/src/utils/mod.rs` (re-exports)
- Modify: `crates/api/src/routes/scripts.rs`
- Modify: `crates/api/src/routes/search.rs`
- Modify: `crates/api/src/routes/cells.rs`
- Modify: `crates/api/src/warmup.rs`

- [ ] **Step 1: Rewrite core resolution in script_resolution.rs**

Replace `resolve_script_version_by_reference()` and `list_script_reference_variants()` with a new cell-index-based resolution function:

```rust
/// Resolve a script hash to a version using cell indexes.
/// Checks type reference first (cell_by_type), then data reference
/// (CF_SCRIPT_INFO existence), then direct version lookup.
pub fn resolve_script_by_hash(
    store: &CkbadgerStore,
    cells_store: &CkbadgerStore,
    reference_hash: &[u8],
) -> Result<CurrentScriptVersionResolution> {
    // 1. Type reference: code cells whose type_script_hash matches
    let type_matches = resolve_live_type_reference_matches(
        store, cells_store, reference_hash,
    )?;
    if !type_matches.is_empty() {
        let unique_versions: Vec<Vec<u8>> = type_matches.iter()
            .map(|m| m.version_hash.clone())
            .collect::<HashSet<_>>().into_iter().collect();
        if unique_versions.len() == 1 {
            let vh = &unique_versions[0];
            let vi = store.get_script_version(vh)?;
            return Ok(Resolved(Box::new(CurrentScriptVersion {
                version_hash: vh.clone(), version_info: vi,
            })));
        }
        return Ok(Ambiguous(Box::new(
            AmbiguousCurrentScriptVersion { version_hashes: unique_versions }
        )));
    }
    // 2. Data-family: version_hash = reference_hash
    if store.get_script_info(reference_hash)?.is_some() {
        let vi = store.get_script_version(reference_hash)?;
        return Ok(Resolved(Box::new(CurrentScriptVersion {
            version_hash: reference_hash.to_vec(), version_info: vi,
        })));
    }
    // 3. Direct version_hash lookup (from labels)
    if store.get_script_version(reference_hash)?.is_some() {
        return Ok(Resolved(Box::new(CurrentScriptVersion {
            version_hash: reference_hash.to_vec(),
            version_info: store.get_script_version(reference_hash)?,
        })));
    }
    Ok(NotFound)
}
```

Delete:
- `ScriptReferenceVariant` struct (lines 39-43)
- `list_script_reference_variants()` (lines 304-319) — reads deleted CF
- `list_current_references_for_version()` (~line 443) — reads deleted CF
- `resolve_script_version_by_reference()` (lines 367-413) — replaced by `resolve_script_by_hash()`

Simplify:
- `AmbiguousCurrentScriptVersion`: remove `available_references` and `type_matches` fields, keep only `version_hashes`
- `CurrentScriptVersion`: remove `available_references` field

Keep:
- `resolve_live_type_reference_matches()` — uses cell indexes, still needed
- `list_version_code_cells()` — uses cell indexes, still needed
- `merge_script_info_for_reference()` — operates on CF_SCRIPT_INFO (NOT deleted CFs), still used by cells.rs

Remove test: `test_resolve_script_version_by_reference_returns_ambiguity_for_live_type_conflict` (calls `put_script_reference` at line 556). Add new test for `resolve_script_by_hash()` using cell index fixtures.

- [ ] **Step 2: Update utils/mod.rs re-exports**

Remove re-exports of deleted functions: `list_current_references_for_version`, `resolve_script_version_by_reference`, `ScriptReferenceVariant`. Add re-export of `resolve_script_by_hash`.

- [ ] **Step 3: Update scripts.rs route handlers**

- Remove `ScriptReferenceResponse` struct and `reference_to_response()` function
- Remove `available_references` from all response builders (lines 154, 211, 928)
- Replace calls to `resolve_script_version_by_reference()` with `resolve_script_by_hash()`
- Replace calls to `list_current_references_for_version()` (lines 483, 1176) — either remove the feature or derive from cell indexes at query time
- Source stats from `CF_SCRIPT_INFO` instead of `CF_SCRIPT_VERSIONS` stats fields

- [ ] **Step 4: Update search.rs**

Replace `resolve_script_version_by_reference()` call (lines 404-438) with `resolve_script_by_hash()`. The `requested_hash_type` parameter was always `None` from search.rs, so the simplified signature works.

- [ ] **Step 5: Verify cells.rs still compiles**

Check that `merge_script_info_for_reference()` (called at lines 1951, 2212) still works. This function operates on `ScriptInfo` from CF_SCRIPT_INFO and should be unaffected. No changes needed if the function signature is unchanged.

- [ ] **Step 6: Update warmup.rs**

Delete:
- `CACHE_KEY_SCRIPT_REFERENCES_BY_HASH` constant (line 32)
- `list_script_references()` call and cache population (lines 424, 439)
- Any code that reads the removed cache key

- [ ] **Step 7: Verify compilation**

Run: `cargo check -p ckbadger-api`
Expected: Compiles

- [ ] **Step 8: Commit**

```
git add crates/api/
git commit -m "refactor: rewrite API script resolution to use cell indexes

Replace CF_SCRIPT_REFERENCES-based resolution with cell-index derivation.
Type references resolved via cell_by_type -> data_hash. Data references
resolved directly via CF_SCRIPT_INFO. Stats from CF_SCRIPT_INFO.
Update scripts.rs, search.rs, warmup.rs. Keep merge_script_info_for_reference
(operates on CF_SCRIPT_INFO, unaffected).

Part of script model simplification (docs/plans/2026-03-17)."
```

---

### Task 4: Delete store layer for removed CFs

All consumers are now removed. Delete CF definitions, accessors, store ops, batch methods, key encoders, and types.

**Files:**
- Modify: `crates/ckbadger-store/src/store.rs`
- Modify: `crates/ckbadger-store/src/batch.rs`
- Modify: `crates/ckbadger-store/src/cell_ops.rs`
- Modify: `crates/ckbadger-store/src/stats_ops.rs`
- Modify: `crates/ckbadger-store/src/keys.rs`
- Modify: `crates/ckbadger-store/src/types.rs`
- Modify: `crates/ckbadger-store/src/lib.rs`

- [ ] **Step 1: Remove CF definitions and accessors from store.rs**

Delete:
- `CF_SCRIPT_REFERENCES` constant (line 314)
- `CF_CELL_SCRIPT_VERSIONS` constant (line 317)
- Remove from `ALL_CFS` array (lines 392, 395)
- Remove from `DOMAIN_CFS` array (lines 452, 455)
- Remove from any CF classification arrays (MEGA_WRITE_CFS etc.) if present
- `cf_script_references()` accessor (lines 1208-1209)
- `cf_cell_script_versions()` accessor (lines 1217-1218)
- Test `test_mega_write_cfs_excludes_script_reference_cfs` (lines 2476-2488)

- [ ] **Step 2: Remove batch operations from batch.rs**

Delete:
- `put_script_reference()` (lines 1064-1072)
- `put_cell_script_version()` (lines 1090-1098)
- `delete_cell_script_version()` (lines 1101-1103)

- [ ] **Step 3: Remove store operations**

From `cell_ops.rs`:
- `get_cell_script_version()` (lines 759-778)
- `get_cell_script_versions_batch()` (lines 782-823)
- `put_cell_script_version()` store method (lines 826-834)
- `test_cell_script_version_roundtrip` test (lines 1209-1225)

From `stats_ops.rs`:
- `get_script_reference()` (lines 843-862)
- `put_script_reference()` (lines 865-873)
- `list_script_references_by_hash()` (lines 876-903)
- `list_script_references()` (lines 906-929)
- `test_list_script_references_returns_all_variants` test (~line 1312)

- [ ] **Step 4: Remove key encoders from keys.rs**

Delete:
- `SCRIPT_REFERENCE_KEY_SIZE` and `SCRIPT_REFERENCE_PREFIX_SIZE` constants
- `encode_script_reference_key()` (~line 38)
- `encode_script_reference_prefix()` (~line 53)
- `decode_script_reference_key()` (~line 64)
- `test_script_reference_key_roundtrip` test (~line 1374)

- [ ] **Step 5: Remove types from types.rs**

Delete:
- `ScriptReferenceInfo` struct (lines 675-694)
- `CellScriptVersionInfo` struct (lines 722-731)

- [ ] **Step 6: Clean up lib.rs re-exports**

Remove explicit re-exports of `CF_CELL_SCRIPT_VERSIONS` and `CF_SCRIPT_REFERENCES` from `crates/ckbadger-store/src/lib.rs` (lines ~71, ~76). Deleted types (`ScriptReferenceInfo`, `CellScriptVersionInfo`) are handled by blanket `pub use types::*`.

- [ ] **Step 7: Fix API integration tests**

In `crates/api/tests/api_integration.rs`: replace all `put_script_reference()` calls (lines 2888, 3002, 3139, 3232, 3340, 3443, 3706, 3715, 3724, 3733) with appropriate cell index fixtures. Tests that check script resolution should create cells with correct lock/type scripts and use `put_script_info()` for stats.

- [ ] **Step 8: Fix dotbit.rs test if needed**

Check `crates/indexer/src/db/writer/dotbit.rs` for calls to deleted batch methods. Remove or adapt.

- [ ] **Step 9: Verify full compilation and tests**

Run: `cargo check && cargo clippy && cargo test`
Expected: Clean compilation, no clippy warnings for deleted code, all tests pass

- [ ] **Step 10: Commit**

```
git add crates/ckbadger-store/ crates/api/tests/ crates/indexer/src/db/writer/dotbit.rs
git commit -m "refactor: delete CF_CELL_SCRIPT_VERSIONS and CF_SCRIPT_REFERENCES

Remove 2 column families, their accessors, batch methods, store operations,
key encoders, and type definitions. Fix API integration tests and dotbit
test to use cell indexes instead of deleted CFs. Domain CFs: 55 -> 53.

Part of script model simplification (docs/plans/2026-03-17)."
```

---

## Chunk 3: Frontend, Docs, Verification

### Task 5: Update frontend

**Files:**
- Modify: `frontend/lib/api.ts`
- Modify: `frontend/app/scripts/[name]/client-page.tsx`
- Modify: `frontend/__tests__/routes/detail-route-inputs.test.tsx`
- Modify: `frontend/__tests__/pages/script-code-hash.test.tsx`

- [ ] **Step 1: Remove availableReferences from TypeScript types**

In `frontend/lib/api.ts`: remove `availableReferences` field from response types, remove `ScriptReferenceOption` interface (if present).

- [ ] **Step 2: Update script detail page**

In `frontend/app/scripts/[name]/client-page.tsx` (lines 249-251, 784-790): remove the "Available References" rendering section that depends on `availableReferences` data.

- [ ] **Step 3: Fix frontend tests**

Update fixtures in:
- `frontend/__tests__/routes/detail-route-inputs.test.tsx` (2 occurrences)
- `frontend/__tests__/pages/script-code-hash.test.tsx` (4 occurrences)

Remove `availableReferences` from test fixture objects.

- [ ] **Step 4: Verify frontend**

Run: `cd frontend && pnpm type-check && pnpm lint && pnpm test`
Expected: All pass

- [ ] **Step 5: Commit**

```
git add frontend/
git commit -m "frontend: remove availableReferences from script pages

Backend no longer returns availableReferences (CF_SCRIPT_REFERENCES deleted).
Remove from types, detail page rendering, and test fixtures."
```

---

### Task 6: Update documentation

**Files:**
- Modify: `docs/STORE_SCHEMA.md`
- Modify: `CLAUDE.md`
- Modify: `docs/SCRIPTS_CODE_CELLS_AND_REFS.md`

- [ ] **Step 1: Update STORE_SCHEMA.md**

Remove CF_CELL_SCRIPT_VERSIONS and CF_SCRIPT_REFERENCES entries. Update CF count in header (55 → 53 domain). Update or remove "Script Modeling Note" section.

- [ ] **Step 2: Update CLAUDE.md**

Update CF count references to "53 domain + 1 append-only". Update any other references to deleted CFs.

- [ ] **Step 3: Update SCRIPTS_CODE_CELLS_AND_REFS.md**

Note that resolution is API query-time from cell indexes, not indexer-time from dedicated CFs. Remove references to CF_CELL_SCRIPT_VERSIONS and CF_SCRIPT_REFERENCES. Keep conceptual model (reference vs version vs code cell) — still correct.

- [ ] **Step 4: Commit**

```
git add docs/ CLAUDE.md
git commit -m "docs: update schema and architecture for script model simplification

CF count 55 -> 53. Remove CF_CELL_SCRIPT_VERSIONS and CF_SCRIPT_REFERENCES
from schema. Script resolution now at API query time from cell indexes."
```

---

### Task 7: Performance verification

- [ ] **Step 1: Build release binary**

Run: `cargo build -p ckbadger --release`

- [ ] **Step 2: Purge and re-sync**

```
ckbadger purge
ckbadger run
```

Monitor sync. Target: ≥ 7000 blk/s, ≤ 45 min wall clock.

- [ ] **Step 3: Check perf artifacts**

After sync: `temp/perf/bulk-sync/latest/report.md`
- `blocks_per_sec_wall` ≥ 7000
- `avg_batch_seconds` ≤ 0.6s

- [ ] **Step 4: Verify API**

```
curl http://localhost:8101/scripts | jq '.data | length'
curl http://localhost:8101/scripts/secp256k1-blake160 | jq '.name'
```

- [ ] **Step 5: Run verify**

Run: `ckbadger verify --depth fast`
Expected: All checks pass
