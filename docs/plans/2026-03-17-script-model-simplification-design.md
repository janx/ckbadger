# Script Model Simplification Design

## Goal

Eliminate the script reference/version indexer-time computation that caused a 48% bulk sync regression (7041 → 3688 blk/s). Move script identity resolution to the API layer, using existing cell indexes and CF_SCRIPT_INFO.

## Principle Alignment

- **CKB Native**: Script identity model remains correct — reference vs version vs code cell distinction preserved in API resolution logic
- **Local First**: Restores bulk sync to baseline speed; rebuild remains cheap
- **Agent Friendly**: Fewer CFs, fewer indexer code paths, simpler mental model

## Problem

Commit `e157e857` ("Add canonical script reference version model") added per-cell script version tracking to the indexer hot path:

1. **CF_CELL_SCRIPT_VERSIONS**: One write per output cell (95.8M entries, ~16.8GB WriteBatch)
2. **Parser dep cell resolution**: `build_script_reference_version_state()` resolves transaction dep cells for every batch (~400 lines, cross-batch cache, CKB store fallback)
3. **Writer stats update**: `update_script_reference_version_batch()` reads + writes CF_SCRIPT_REFERENCES and CF_SCRIPT_VERSIONS

This infrastructure exists to maintain a decrement path: when a cell is consumed, read back its stored version info to update aggregate stats. But:

- **API never reads CF_CELL_SCRIPT_VERSIONS** — only the indexer reads it during consumption
- **CF_SCRIPT_INFO (legacy) already maintains per-code_hash live stats** — the same data the API needs
- **Script resolution can be derived from existing cell indexes** at query time

The result: 48% sync regression, 8 follow-up bug fixes for dep cell resolution edge cases, and ~2000 lines of complex code serving a nice-to-have feature.

## Design

### Core Principle

**Indexer stores raw data and generic indexes. API interprets and resolves.**

Script identity correctness moves from indexer-time pre-computation to API query-time derivation. The indexer continues to maintain cell indexes and CF_SCRIPT_INFO (as it already does), but does no script-version-specific work.

### Indexer Changes: Delete

**Parser hot path — remove entirely:**

| Code | Location | Purpose |
|------|----------|---------|
| `build_script_reference_version_state()` | `sync/batch.rs` | Resolve dep cells, build version changes |
| `parser_version_cache` | `sync/pipeline.rs` | Cross-batch version cache |
| `prune_parser_version_cache()` | `sync/pipeline.rs` | Cache eviction |
| `reset_parser_version_cache_epoch()` | `sync/pipeline.rs` | Cache reset on reorg |
| `parse_cell_deps()` call | `parser/block.rs` | Parse cell deps per tx |
| `DepCellCache` | `sync/batch.rs` | Dep cell dedup cache |
| `resolve_tx_dep_cells()` | `sync/batch.rs` | Dep cell resolution chain |
| `load_dep_cell_info()` | `sync/batch.rs` | Single dep cell load |
| `load_dep_cell_data()` | `sync/batch.rs` | Dep cell data load |
| `script_version_ms` timing | `pipeline.rs`, `bulk_sync_perf.rs` | Perf instrumentation |

**Writer T1 — remove:**

| Code | Location | Purpose |
|------|----------|---------|
| `put_cell_script_version()` loop | `sync/batch.rs:2569-2571` | Per-cell version write |
| `cell_script_version_rows` field | `ParsedBatch`, `sync/types.rs` | Batch data carrier |

**Writer T2 — remove:**

| Code | Location | Purpose |
|------|----------|---------|
| `update_script_reference_version_batch()` call | `sync/batch.rs:2657-2661` | Reference/version stats |
| `update_script_reference_version_batch()` | `db/writer/addresses.rs` | Implementation |
| `read_script_references()` | `db/writer/addresses.rs` | Multi-get for delta apply |
| `read_script_versions()` | `db/writer/addresses.rs` | Multi-get for delta apply |
| `apply_script_reference_version_deltas()` | `db/writer/addresses.rs` | Delta application |
| `script_reference_version_changes` field | `ParsedBatch`, `sync/types.rs` | Batch data carrier |

**Bug fix code that becomes dead:**

| Commit | Description |
|--------|-------------|
| `2722adb4` | CKB store fallback for dep cell info |
| `3a59df3a` | TYPE_ID built-in script handling |
| `a69b63e5` | Parser version cache reset |
| `64c32be9` | Unresolved output lock type references |
| `aa93a743` | Parser cache committed_tip init |

**Reorg rollback — remove script reference/version delta path:**

| Code | Location | Purpose |
|------|----------|---------|
| `accumulate_script_reference_version_deltas()` | `reorg_ops.rs` | Build deltas from rolled-back cells |
| `get_cell_script_version()` calls (5 sites) | `reorg_ops.rs` | Read per-cell version during rollback |
| Script reference/version delta application loop | `reorg_ops.rs` | Apply deltas to CF_SCRIPT_REFERENCES/CF_SCRIPT_VERSIONS |
| `delete_cf(cf_cell_script_versions())` | `reorg_ops.rs:184` | Delete cell index entry during rollback |

CF_SCRIPT_INFO reorg path (`accumulate_cell_deltas()` / `apply_script_usage_deltas()`) is independent and remains unchanged.

**Store layer — remove operations for deleted CFs:**

| Code | Location | Purpose |
|------|----------|---------|
| `get_cell_script_version()` | `cell_ops.rs` | Point read |
| `get_cell_script_versions_batch()` | `cell_ops.rs` | Multi-get |
| `put_cell_script_version()` | `cell_ops.rs` | Direct write |
| `get_script_reference()`, `put_script_reference()` | `stats_ops.rs` | Point read/write |
| `list_script_references_by_hash()`, `list_script_references()` | `stats_ops.rs` | Iterators |
| `put_script_reference()`, `put_cell_script_version()`, `delete_cell_script_version()` | `batch.rs` | Batch operations |
| `cf_cell_script_versions()`, `cf_script_references()` | `store.rs` | CF accessors |
| `ScriptReferenceInfo` type | `types.rs` | Struct definition |
| `test_cell_script_version_roundtrip` | `cell_ops.rs` | Test |

**API warmup — remove CF_SCRIPT_REFERENCES cache:**

| Code | Location | Purpose |
|------|----------|---------|
| `list_script_references()` call | `warmup.rs` | Warmup cache for script references |
| `CACHE_KEY_SCRIPT_REFERENCES_BY_HASH` | `warmup.rs` | Cache key |

**Test fixtures using deleted CFs:**

| Code | Location | Purpose |
|------|----------|---------|
| `put_script_reference()` calls (9+ sites) | `api_integration.rs` | API test fixtures |
| `put_cell_script_version()` call | `db/writer/dotbit.rs` | DotBit test fixture |

### Column Family Changes

| CF | Action | Reason |
|----|--------|--------|
| `CF_CELL_SCRIPT_VERSIONS` | **Delete** | Only reader was indexer's own `build_script_reference_version_state()` |
| `CF_SCRIPT_REFERENCES` | **Delete** | Stats covered by CF_SCRIPT_INFO; hash_type derivable from cell indexes |
| `CF_SCRIPT_VERSIONS` | **Keep** | Metadata (name, category) from `label_import`. Stats fields zeroed (not maintained by indexer); API sources stats from CF_SCRIPT_INFO instead |
| `CF_SCRIPT_VERSIONS_BY_LABEL` | **Keep** | Label→version index from `label_import` |
| `CF_SCRIPT_INFO` | **Keep, unchanged** | Already maintained by T2 `apply_script_usage_deltas()`. Provides live stats |

Domain CFs: 55 → 53. (Note: CLAUDE.md says 51 — update all doc references to correct count.)

### API Resolution Changes

**`/script/{hash}` — resolve a script hash:**

Current: read CF_SCRIPT_REFERENCES → filter by hash_type → read CF_SCRIPT_VERSIONS

New:
1. Check `CF_SCRIPT_INFO[hash]` — existence and lock/type usage
2. Check `CF_SCRIPT_VERSIONS[hash]` — label metadata (hash may be a version_hash)
3. Derive hash_type from cell indexes:
   - `cell_by_type(hash)` has results → type reference, live code cells reveal version_hash via data_hash
   - Otherwise → data-family reference, version_hash = hash
4. `cell_by_data_hash(version_hash)` → list code cell instances
5. Stats from `CF_SCRIPT_INFO[hash]`

**`/scripts/{name}` — by label:**

Unchanged flow: `CF_SCRIPT_VERSIONS_BY_LABEL[name]` → `CF_SCRIPT_VERSIONS[version_hash]` → metadata. Stats from `CF_SCRIPT_INFO`.

**`/scripts` — list page:**

Current: enumerate CF_SCRIPT_VERSIONS + merge CF_SCRIPT_REFERENCES.
New: enumerate `CF_SCRIPT_INFO` + merge label metadata from `CF_SCRIPT_VERSIONS`. Simpler.

**`/scripts/lookup` — bulk lookup:**

Same pattern as `/script/{hash}` but batched. Cache hot script resolutions in memory.

**Performance:** Single `/script/{hash}` query: 2-4 point reads + 1 prefix scan. Millisecond-level. Hot scripts cached via existing warmup infrastructure.

**Simplification:** Remove `ScriptReferenceVariant`, `AmbiguousCurrentScriptVersion` types. Simplify `CurrentScriptVersionResolution` to use cell-index-based derivation. Remove `merge_script_info_for_reference()` complexity.

### What Stays Unchanged

- **Cell indexes**: CF_CELL_BY_LOCK, CF_CELL_BY_TYPE, CF_CELL_BY_LOCK_CODE, CF_CELL_BY_TYPE_CODE — already maintained, zero new cost
- **CF_SCRIPT_INFO**: Already maintained by T2 `apply_script_usage_deltas()` — live cells count, capacity stats
- **label_import**: Continues to populate CF_SCRIPT_VERSIONS and CF_SCRIPT_VERSIONS_BY_LABEL
- **Reorg handling**: CF_SCRIPT_INFO reorg path (`accumulate_cell_deltas` / `apply_script_usage_deltas`) already exists independently
- **Live sync**: No changes needed. CF_SCRIPT_INFO maintenance continues as-is
- **Frontend**: Core functionality unchanged. Remove `availableReferences` field from responses (was sourced from CF_SCRIPT_REFERENCES)
- **Verify checks**: No existing verify checks reference the deleted CFs — zero verify changes needed

### Migration

- Delete CF definitions from store.rs
- `ckbadger purge` + re-sync from genesis
- No backward compatibility needed (project policy: schema changes are cheap)

## Validation

- `cargo test` — all existing tests pass (remove dead tests, rewrite API integration fixtures to use cell indexes instead of CF_SCRIPT_REFERENCES)
- `cargo clippy` — clean
- Bulk sync perf run: target ≥ 7000 blk/s (restore baseline)
- API integration tests: `/script/{hash}` resolution returns correct results using cell-index-based derivation
- No verify check changes needed (no existing checks reference deleted CFs)

## Performance Recovery Estimate

| Metric | Current (a69b63e5) | Expected | Source |
|--------|-------------------|----------|--------|
| Bulk sync blk/s | 3688 | ≥7000 | Eliminate 95.8M writes + dep cell resolution |
| Wall clock (18.8M blocks) | 85 min | ≤45 min | Restore baseline |
| Parser total | 3933s | ~800s | Remove script_version computation |
| Writer batch_seconds | 4264s | ~2500s | Smaller WriteBatch, less compaction |
| API `/script/{hash}` | ~1ms | ~2-3ms | Cell index lookup (cacheable) |

## Scope

- **Files changed**: ~25 (indexer sync/parser/writer/pipeline, store CFs/ops/types/batch, API routes/resolution/warmup, reorg_ops, tests)
- **Lines deleted**: ~2000-2500 (parser script version code, writer, pipeline cache, store ops, reorg delta path, dead tests)
- **Lines modified**: ~300-500 (API resolution logic, warmup, API integration test fixtures)
- **Lines added**: ~0-100 (API cell-index derivation helpers)
- **Storage impact**: -2 CFs, ~16.8GB less WriteBatch per sync
- **Docs to update**: STORE_SCHEMA.md, CLAUDE.md (CF count: 55→53), SCRIPTS_CODE_CELLS_AND_REFS.md
- **Re-sync required**: Yes
