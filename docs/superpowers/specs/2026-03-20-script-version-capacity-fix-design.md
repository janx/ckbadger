# Script Version Capacity Fix — Read-Time Merge from ScriptInfo

**Date:** 2026-03-20
**Status:** Draft
**Scope:** Fix missing capacity/common knowledge size on script detail, lookup, and usage API responses

## Problem

After a fresh DB sync, the script detail page (`GET /scripts/{name}`), script lookup
(`POST /scripts/lookup`), and script usage (`GET /scripts/{name}/usage`) endpoints return
`0` for all capacity and common knowledge size fields.

**Root cause:** These handlers read capacity from `ScriptVersionInfo` (stored in
`CF_SCRIPT_VERSIONS`), but that table's capacity fields are never populated.
`ScriptVersionInfo` is only written by label import (`label_import.rs`), which sets
metadata (name, category, description, website) and leaves all capacity fields at their
default of `0`.

Neither the bulk-build engine nor the live-sync pipeline updates `ScriptVersionInfo`
capacity fields. The correct capacity data exists in `ScriptInfo` (`CF_SCRIPT_INFO`),
which IS populated by the bulk-build's `ScriptOwner::materialize_final()` and updated
by the pipeline's `apply_script_usage_deltas()`.

**Note:** The script *list* page (`GET /scripts`) works correctly because it reads from
`ScriptInfo` directly via `script_info_to_response()`.

**Note:** `resolve_script_identifier` → `fallback_script_version_info` does merge
`ScriptInfo` capacity into `ScriptVersionInfo`, but only when the `ScriptVersionInfo`
entry is **missing** from `CF_SCRIPT_VERSIONS`. For labeled scripts (the common case),
label import writes an entry with zero capacity, so the fallback is skipped and zeros
pass through.

## Key Constraint: version_hash vs code_hash

`CF_SCRIPT_VERSIONS` is keyed by `version_hash`, which is the `data_hash` of the
deployment cell (from label import). `CF_SCRIPT_INFO` is keyed by `code_hash` (the hash
used in cell scripts).

- **Data-ref scripts** (hash_type=data/data1/data2): `code_hash == data_hash`, so
  `get_script_info(&version_hash)` returns the correct `ScriptInfo`.
- **Type-ref scripts** (hash_type=type): `code_hash` is the `type_hash` of the deployment
  cell, which differs from `data_hash`. `get_script_info(&version_hash)` returns `None`.

For type-ref scripts, a fallback lookup by name from the cached `ScriptInfo` list is
needed.

## Solution

One unified helper `resolve_version_capacity` replaces all `version_totals()` calls
in the affected handlers. It uses a two-tier lookup with fail-fast validation:

1. Try `get_script_info(&version_hash)` (direct hit for data-ref scripts)
2. If None, search cached `ScriptInfo` entries by name (handles type-ref scripts;
   for scripts with multiple deployments under the same name, the first match is
   used — a best-effort approximation that is better than the current zero)
3. Fall back to `version_totals()` (zeros) if neither produces a result

This avoids adding a new write path to `ScriptVersionInfo` — no dual-update risk, no
schema migration, no re-sync required.

## Changes

### 1. `resolve_version_capacity` helper in `scripts.rs`

```rust
/// Resolve capacity totals for a script version by looking up ScriptInfo.
///
/// Tier 1: direct lookup by version_hash (works for data-ref scripts where
/// version_hash == code_hash).
/// Tier 2: name-based search in cached ScriptInfo (handles type-ref scripts
/// where version_hash is a data_hash, not a code_hash).
/// Tier 3: fall back to ScriptVersionInfo fields (zeros).
///
/// Validates all returned values are non-negative and consistent (used ≤ total),
/// following the checked_capacity_totals pattern.
fn resolve_version_capacity(
    version: &ScriptVersionInfo,
    store: &CkbadgerStore,
    script_infos_cache: &[(Vec<u8>, ScriptInfo)],
) -> Result<(i64, i64, i128, i128, i128, i128), ApiRouteError> {
    // Tier 1: direct lookup
    let script_info = store
        .get_script_info(&version.version_hash)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Tier 2: name-based fallback
    let info = script_info.or_else(|| {
        let name = version.name.as_deref()?;
        script_infos_cache
            .iter()
            .find(|(_, info)| info.name.as_deref() == Some(name))
            .map(|(_, info)| info.clone())
    });

    let Some(info) = info else {
        // Tier 3: no ScriptInfo found
        return Ok(version_totals(version));
    };

    // Validate live capacity (matches checked_capacity_totals pattern)
    let live_cap = info.lock_live_capacity_sum + info.type_live_capacity_sum;
    let live_used = info.lock_live_used_capacity_sum + info.type_live_used_capacity_sum;
    if live_cap < 0 {
        return Err(ApiError::internal(format!(
            "negative live capacity: code_hash=0x{}, value={}",
            hex::encode(&info.code_hash), live_cap
        )));
    }
    if live_used < 0 {
        return Err(ApiError::internal(format!(
            "negative live used capacity: code_hash=0x{}, value={}",
            hex::encode(&info.code_hash), live_used
        )));
    }
    if live_used > live_cap {
        return Err(ApiError::internal(format!(
            "live used exceeds total: code_hash=0x{}, used={}, total={}",
            hex::encode(&info.code_hash), live_used, live_cap
        )));
    }

    Ok((
        info.lock_cells_count + info.type_cells_count,
        info.lock_live_cells_count + info.type_live_cells_count,
        info.lock_capacity_sum + info.type_capacity_sum,
        info.lock_live_capacity_sum + info.type_live_capacity_sum,
        info.lock_used_capacity_sum + info.type_used_capacity_sum,
        info.lock_live_used_capacity_sum + info.type_live_used_capacity_sum,
    ))
}
```

### 2. `get_script` handler (~line 1127)

Load `script_infos_cache` once before the loop. Replace `version_totals(&version_info)`
with `resolve_version_capacity(&version_info, &state.store, &all_script_infos)?`.

The existing `script_info` read at line 1148 stays for hash_type/deployment resolution.

### 3. `lookup_scripts` handler (~line 740)

Load `script_infos_cache` once. Replace `version_totals(&version_info)` at line 755
with `resolve_version_capacity(&version_info, &state.store, &all_script_infos)?`.

Also consolidate the two redundant `get_script_info(code_hash)` calls at lines 761
and 769 into one shared read. This is a cleanup regardless of the capacity fix.

### 4. `get_script_usage` handler (~line 1275)

Load `script_infos_cache` once. Replace `version_totals(&info)` at line 1285 with
`resolve_version_capacity(&info, &state.store, &all_script_infos)?`.

The `.map()` closure return type changes from bare value to `Result`, requiring
`.collect::<Result<Vec<_>, _>>()?` instead of `.collect::<Vec<_>>()`.

The existing `i128 as u128` casts at lines 1286-1289 are safe because
`resolve_version_capacity` validates non-negative values via the fail-fast checks.

## What does NOT change

- `ScriptVersionInfo` struct — no new fields, no schema change
- `CF_SCRIPT_VERSIONS` write paths — label import remains the only writer
- `ScriptInfo` / `CF_SCRIPT_INFO` — already correct, no changes needed
- `ScriptOwner` (bulk build) — already correct
- `apply_script_usage_deltas` (pipeline) — already correct
- Script list page (`list_scripts`) — already reads from `ScriptInfo`
- Script capacity history chart — already reads from `CF_STATS_SCRIPT` daily deltas
- `fallback_script_version_info` — unchanged (still used for unlabeled scripts)

## Store Boundary

- Read-only change for API: reads existing `CF_SCRIPT_INFO` data
- No new writes to any CF
- No append-only store involvement
- Domain vs append-only target: N/A (no writes)

## Testing

- Unit test: `resolve_version_capacity` with ScriptInfo found by direct version_hash
  lookup (data-ref case) — verify non-zero capacity returned.
- Unit test: `resolve_version_capacity` with no direct hit but name-matched ScriptInfo
  (type-ref case) — verify capacity from name-matched entry.
- Unit test: `resolve_version_capacity` with no ScriptInfo at all — verify falls back
  to version_totals zeros.
- Unit test: `resolve_version_capacity` rejects negative live capacity (fail-fast).
- Unit test: `resolve_version_capacity` rejects used > total (fail-fast).
- Manual verification: after sync, `GET /scripts/SECP256K1_BLAKE160` (type-ref) and
  `GET /scripts/anyone_can_pay` (data-ref) both return non-zero `liveCapacitySum` and
  `liveCommonKnowledgeSizeSum`.

## Performance Impact

- `get_script`: loads `script_infos_cache` once (already cached in memory by warmup),
  one `get_script_info` DB read per deployment inside `resolve_version_capacity`.
  Net zero vs current — the existing code already reads `script_info` per deployment.
- `lookup_scripts`: net reduction — consolidates three `get_script_info` reads into one
  per code_hash (the one inside `resolve_version_capacity`, plus shared reuse for
  hash_type and deployment resolution).
- `get_script_usage`: loads `script_infos_cache` once, one `get_script_info` read per
  deployment (1-3 typically). New reads, but minimal.
- No impact on indexer or bulk-build performance.
