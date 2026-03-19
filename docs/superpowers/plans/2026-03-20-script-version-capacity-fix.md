# Script Version Capacity Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix script detail, lookup, and usage API endpoints returning 0 for capacity and common knowledge size by merging capacity from ScriptInfo at read time.

**Architecture:** Add `resolve_version_capacity` helper that does a two-tier ScriptInfo lookup (direct by version_hash, then name-based fallback from cache). Replace `version_totals()` calls in three handlers with this helper. Includes fail-fast validation matching existing `checked_capacity_totals` pattern.

**Tech Stack:** Rust, Axum 0.8, ckbadger-store (RocksDB)

**Spec:** `docs/superpowers/specs/2026-03-20-script-version-capacity-fix-design.md`

---

## File Structure

| File | Action | Responsibility |
|------|--------|----------------|
| `crates/api/src/routes/scripts.rs` | Modify | Add `resolve_version_capacity` helper; update 3 handlers |

All changes are in a single file. The helper is private to the scripts route module.

---

### Task 1: Add `resolve_version_capacity` helper with unit tests

**Files:**
- Modify: `crates/api/src/routes/scripts.rs` (add helper function ~line 540, add tests in `mod tests`)

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block (after existing `checked_capacity_totals` tests, ~line 1720). Import `ScriptVersionInfo` and the new function in the test module's `use super::` block.

```rust
// In the `use super::` import block at line 1664, add:
//   resolve_version_capacity,
// In the `use` section, add:
//   use ckbadger_store::types::ScriptVersionInfo;

#[test]
fn resolve_version_capacity_uses_direct_script_info_parameter() {
    let version = ScriptVersionInfo {
        version_hash: vec![0xAA; 32],
        name: Some("test_script".to_string()),
        ..Default::default()
    };
    let script_info = ScriptInfo {
        code_hash: vec![0xAA; 32],
        lock_live_capacity_sum: 100_00000000,
        lock_live_used_capacity_sum: 61_00000000,
        lock_cells_count: 5,
        lock_live_cells_count: 3,
        lock_capacity_sum: 200_00000000,
        lock_used_capacity_sum: 122_00000000,
        ..Default::default()
    };
    // Pass ScriptInfo directly (Tier 1 path — caller pre-loaded it)
    let cache: Vec<(Vec<u8>, ScriptInfo)> = vec![];

    let (cells, live, cap, live_cap, used, live_used) =
        resolve_version_capacity(&version, Some(&script_info), &cache).unwrap();
    assert_eq!(cells, 5);
    assert_eq!(live, 3);
    assert_eq!(cap, 200_00000000);
    assert_eq!(live_cap, 100_00000000);
    assert_eq!(used, 122_00000000);
    assert_eq!(live_used, 61_00000000);
}

#[test]
fn resolve_version_capacity_finds_by_version_hash_in_cache() {
    let version = ScriptVersionInfo {
        version_hash: vec![0xAA; 32],
        name: Some("test_script".to_string()),
        ..Default::default()
    };
    let script_info = ScriptInfo {
        code_hash: vec![0xAA; 32],
        lock_live_capacity_sum: 100_00000000,
        lock_live_used_capacity_sum: 61_00000000,
        lock_cells_count: 5,
        lock_live_cells_count: 3,
        lock_capacity_sum: 200_00000000,
        lock_used_capacity_sum: 122_00000000,
        ..Default::default()
    };
    // Cache has entry keyed by code_hash matching version_hash (Tier 1b path)
    let cache = vec![(vec![0xAA; 32], script_info)];

    let (cells, live, cap, live_cap, used, live_used) =
        resolve_version_capacity(&version, None, &cache).unwrap();
    assert_eq!(cells, 5);
    assert_eq!(live, 3);
    assert_eq!(cap, 200_00000000);
    assert_eq!(live_cap, 100_00000000);
    assert_eq!(used, 122_00000000);
    assert_eq!(live_used, 61_00000000);
}

#[test]
fn resolve_version_capacity_falls_back_to_name_match() {
    // version_hash (data_hash) differs from code_hash (type-ref script)
    let version = ScriptVersionInfo {
        version_hash: vec![0xBB; 32], // data_hash
        name: Some("secp256k1_blake160".to_string()),
        ..Default::default()
    };
    let script_info = ScriptInfo {
        code_hash: vec![0xCC; 32], // type_hash (different!)
        name: Some("secp256k1_blake160".to_string()),
        lock_live_capacity_sum: 500_00000000,
        lock_live_used_capacity_sum: 200_00000000,
        lock_cells_count: 10,
        lock_live_cells_count: 8,
        lock_capacity_sum: 800_00000000,
        lock_used_capacity_sum: 400_00000000,
        ..Default::default()
    };
    // Cache has the ScriptInfo under its code_hash (0xCC), not version_hash (0xBB)
    let cache = vec![(vec![0xCC; 32], script_info)];

    let (cells, live, _cap, live_cap, _used, live_used) =
        resolve_version_capacity(&version, None, &cache).unwrap();
    assert_eq!(cells, 10);
    assert_eq!(live, 8);
    assert_eq!(live_cap, 500_00000000);
    assert_eq!(live_used, 200_00000000);
}

#[test]
fn resolve_version_capacity_returns_zeros_when_no_script_info() {
    let version = ScriptVersionInfo {
        version_hash: vec![0xDD; 32],
        name: Some("unknown_script".to_string()),
        ..Default::default()
    };
    let cache: Vec<(Vec<u8>, ScriptInfo)> = vec![];

    let (cells, live, cap, live_cap, used, live_used) =
        resolve_version_capacity(&version, None, &cache).unwrap();
    assert_eq!(cells, 0);
    assert_eq!(live, 0);
    assert_eq!(cap, 0);
    assert_eq!(live_cap, 0);
    assert_eq!(used, 0);
    assert_eq!(live_used, 0);
}

#[test]
fn resolve_version_capacity_rejects_negative_live_capacity() {
    let version = ScriptVersionInfo {
        version_hash: vec![0xEE; 32],
        name: Some("bad_script".to_string()),
        ..Default::default()
    };
    let bad_info = ScriptInfo {
        code_hash: vec![0xEE; 32],
        lock_live_capacity_sum: -1,
        ..Default::default()
    };
    let cache = vec![(vec![0xEE; 32], bad_info)];

    let err = resolve_version_capacity(&version, None, &cache).unwrap_err();
    assert_eq!(err.0, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(err.1 .0.message.contains("negative live capacity"));
}

#[test]
fn resolve_version_capacity_rejects_used_exceeds_total() {
    let version = ScriptVersionInfo {
        version_hash: vec![0xFF; 32],
        name: Some("bad_script2".to_string()),
        ..Default::default()
    };
    let bad_info = ScriptInfo {
        code_hash: vec![0xFF; 32],
        lock_live_capacity_sum: 100,
        lock_live_used_capacity_sum: 101,
        ..Default::default()
    };
    let cache = vec![(vec![0xFF; 32], bad_info)];

    let err = resolve_version_capacity(&version, None, &cache).unwrap_err();
    assert_eq!(err.0, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(err.1 .0.message.contains("live used exceeds total"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ckbadger-api resolve_version_capacity -- --nocapture`
Expected: FAIL — `resolve_version_capacity` does not exist yet

- [ ] **Step 3: Write the `resolve_version_capacity` helper**

Add after `checked_capacity_totals` (~line 540) in `scripts.rs`:

```rust
/// Resolve capacity totals for a script version by looking up ScriptInfo.
///
/// Tier 1: direct lookup by version_hash in the cache (works for data-ref scripts
/// where version_hash == code_hash).
/// Tier 2: name-based search in cached ScriptInfo (handles type-ref scripts where
/// version_hash is a data_hash, not a code_hash).
/// Tier 3: fall back to ScriptVersionInfo fields (zeros).
fn resolve_version_capacity(
    version: &ckbadger_store::types::ScriptVersionInfo,
    direct_script_info: Option<&ckbadger_store::ScriptInfo>,
    script_infos_cache: &[(Vec<u8>, ckbadger_store::ScriptInfo)],
) -> Result<(i64, i64, i128, i128, i128, i128), ApiRouteError> {
    // Tier 1: use pre-fetched direct lookup (caller may have already loaded it)
    let info = direct_script_info.cloned().or_else(|| {
        // Tier 1b: search cache by version_hash
        script_infos_cache
            .iter()
            .find(|(code_hash, _)| code_hash == &version.version_hash)
            .map(|(_, info)| info.clone())
    }).or_else(|| {
        // Tier 2: name-based fallback for type-ref scripts
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

    // Compute all 6 return values
    let cells = info.lock_cells_count + info.type_cells_count;
    let live_cells = info.lock_live_cells_count + info.type_live_cells_count;
    let cap = info.lock_capacity_sum + info.type_capacity_sum;
    let live_cap = info.lock_live_capacity_sum + info.type_live_capacity_sum;
    let used = info.lock_used_capacity_sum + info.type_used_capacity_sum;
    let live_used = info.lock_live_used_capacity_sum + info.type_live_used_capacity_sum;

    // Validate all capacity values (fail-fast, matches checked_capacity_totals pattern).
    // Total (historical) values are also checked because get_script_usage casts i128→u128.
    if cap < 0 {
        return Err(ApiError::internal(format!(
            "negative capacity in resolve_version_capacity: code_hash=0x{}, value={}",
            hex::encode(&info.code_hash), cap
        )));
    }
    if used < 0 {
        return Err(ApiError::internal(format!(
            "negative used capacity in resolve_version_capacity: code_hash=0x{}, value={}",
            hex::encode(&info.code_hash), used
        )));
    }
    if live_cap < 0 {
        return Err(ApiError::internal(format!(
            "negative live capacity in resolve_version_capacity: code_hash=0x{}, value={}",
            hex::encode(&info.code_hash), live_cap
        )));
    }
    if live_used < 0 {
        return Err(ApiError::internal(format!(
            "negative live used capacity in resolve_version_capacity: code_hash=0x{}, value={}",
            hex::encode(&info.code_hash), live_used
        )));
    }
    if live_used > live_cap {
        return Err(ApiError::internal(format!(
            "live used exceeds total in resolve_version_capacity: code_hash=0x{}, used={}, capacity={}",
            hex::encode(&info.code_hash), live_used, live_cap
        )));
    }

    Ok((cells, live_cells, cap, live_cap, used, live_used))
}
```

**Design notes for the implementing agent:**
- The function takes `direct_script_info: Option<&ScriptInfo>` so callers that already
  loaded ScriptInfo (like `get_script` at line 1148) can pass it in without a redundant
  cache search.
- Tier 1b searches the in-memory cache by version_hash rather than doing a DB read.
  The cache is already loaded by warmup and used by other handlers.
- The `version_totals` function at line 355 is the existing fallback (returns zeros from
  ScriptVersionInfo).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ckbadger-api resolve_version_capacity -- --nocapture`
Expected: All 6 tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/routes/scripts.rs
git commit -m "feat(api): add resolve_version_capacity helper with two-tier ScriptInfo lookup"
```

---

### Task 2: Update `get_script` handler

**Files:**
- Modify: `crates/api/src/routes/scripts.rs:1111-1196` (`get_script` handler)

- [ ] **Step 1: Load script_infos cache and replace `version_totals` call**

In `get_script` (~line 1111), add a cache load before the `for` loop, then replace
the `version_totals` call at line 1133-1140:

**Before** (lines 1117-1140):
```rust
    let matching: Vec<_> = load_script_versions_cached(&state)?
        .into_iter()
        .filter(|(_, info)| info.name.as_deref() == Some(name.as_str()))
        .collect();

    if matching.is_empty() {
        return Err(ApiError::not_found("Script not found"));
    }
    let mut scripts = Vec::new();

    for (version_hash, version_info) in matching {
        let mut code_cells =
            list_version_code_cells(&state.store, &state.append_only_store, &version_hash)
                .map_err(|e| ApiError::internal(e.to_string()))?;
        sort_code_cells(&mut code_cells);

        let (
            cells_count,
            live_cells_count,
            _capacity_sum,
            live_capacity_sum,
            _used_sum,
            live_used_sum,
        ) = version_totals(&version_info);
```

**After:**
```rust
    let matching: Vec<_> = load_script_versions_cached(&state)?
        .into_iter()
        .filter(|(_, info)| info.name.as_deref() == Some(name.as_str()))
        .collect();

    if matching.is_empty() {
        return Err(ApiError::not_found("Script not found"));
    }

    let all_script_infos = load_script_infos_cached(&state)?;
    let mut scripts = Vec::new();

    for (version_hash, version_info) in matching {
        let mut code_cells =
            list_version_code_cells(&state.store, &state.append_only_store, &version_hash)
                .map_err(|e| ApiError::internal(e.to_string()))?;
        sort_code_cells(&mut code_cells);

        // Derive hash_type, type_hash, data_hash from ScriptInfo if available
        let script_info = state.store.get_script_info(&version_hash).ok().flatten();

        let (
            cells_count,
            live_cells_count,
            _capacity_sum,
            live_capacity_sum,
            _used_sum,
            live_used_sum,
        ) = resolve_version_capacity(&version_info, script_info.as_ref(), &all_script_infos)?;
```

Note: the `script_info` load moves UP from line 1148 to before the capacity call. The
existing usages of `script_info` below (hash_type at line 1149, deployment hashes at
line 1152) now reference the same variable — remove the duplicate load at line 1148.

- [ ] **Step 2: Run existing tests**

Run: `cargo test -p ckbadger-api -- --nocapture`
Expected: All existing tests PASS (no behavioral change for the test suite)

- [ ] **Step 3: Run clippy**

Run: `cargo clippy -p ckbadger-api`
Expected: No new warnings

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/routes/scripts.rs
git commit -m "fix(api): use resolve_version_capacity in get_script handler"
```

---

### Task 3: Update `lookup_scripts` handler

**Files:**
- Modify: `crates/api/src/routes/scripts.rs:738-810` (`lookup_scripts` handler)

- [ ] **Step 1: Replace `version_totals` and consolidate redundant reads**

**Before** (lines 748-779):
```rust
                let (
                    _cells_count,
                    live_cells_count,
                    _capacity_sum,
                    live_capacity_sum,
                    _used_sum,
                    live_used_sum,
                ) = version_totals(&version_info);
                let code_cell = code_cells.first();

                // Derive hash_type from ScriptInfo if available
                let hash_type = state
                    .store
                    .get_script_info(code_hash)
                    .ok()
                    .flatten()
                    .and_then(|info| hash_type_to_string(info.hash_type).map(|s| s.to_string()));

                // Derive deployment hashes from ScriptInfo
                let (deployment_type_hash, deployment_data_hash) = state
                    .store
                    .get_script_info(code_hash)
                    .ok()
                    .flatten()
                    .map(|info| {
                        let (type_ref, data_ref) = deployment_reference_hashes(&info);
                        (
                            type_ref.map(|h| format!("0x{}", hex::encode(h))),
                            data_ref.map(|h| format!("0x{}", hex::encode(h))),
                        )
                    })
                    .unwrap_or((None, None));
```

**After:**
```rust
                let code_cell = code_cells.first();

                // Load ScriptInfo once — used for capacity, hash_type, and deployment hashes
                let script_info = state
                    .store
                    .get_script_info(code_hash)
                    .ok()
                    .flatten();

                let all_script_infos = load_script_infos_cached(&state)?;
                let (
                    _cells_count,
                    live_cells_count,
                    _capacity_sum,
                    live_capacity_sum,
                    _used_sum,
                    live_used_sum,
                ) = resolve_version_capacity(&version_info, script_info.as_ref(), &all_script_infos)?;

                let hash_type = script_info
                    .as_ref()
                    .and_then(|info| hash_type_to_string(info.hash_type).map(|s| s.to_string()));

                let (deployment_type_hash, deployment_data_hash) = script_info
                    .as_ref()
                    .map(|info| {
                        let (type_ref, data_ref) = deployment_reference_hashes(info);
                        (
                            type_ref.map(|h| format!("0x{}", hex::encode(h))),
                            data_ref.map(|h| format!("0x{}", hex::encode(h))),
                        )
                    })
                    .unwrap_or((None, None));
```

Note: `load_script_infos_cached` is inside the match arm here. The implementing agent
should hoist it before the `for code_hash` loop to avoid cloning the cached Vec on
each iteration. The `script_info` is loaded by `code_hash` (user's reference hash),
which is the correct key for `CF_SCRIPT_INFO` — it works for both data-ref and type-ref
scripts because the user supplies the code_hash used in cell scripts.

- [ ] **Step 2: Run existing tests**

Run: `cargo test -p ckbadger-api -- --nocapture`
Expected: All existing tests PASS

- [ ] **Step 3: Run clippy**

Run: `cargo clippy -p ckbadger-api`
Expected: No new warnings

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/routes/scripts.rs
git commit -m "fix(api): use resolve_version_capacity in lookup_scripts, consolidate reads"
```

---

### Task 4: Update `get_script_usage` handler

**Files:**
- Modify: `crates/api/src/routes/scripts.rs:1246-1319` (`get_script_usage` handler)

- [ ] **Step 1: Replace `version_totals` and fix `.map()` return type**

**Before** (lines 1267-1309):
```rust
    let mut total_cells: i64 = 0;
    let mut total_live: i64 = 0;
    let mut total_cap: u128 = 0;
    let mut total_live_cap: u128 = 0;
    let mut total_used_cap: u128 = 0;
    let mut total_live_used_cap: u128 = 0;

    let by_deployment: Vec<DeploymentUsage> = matching
        .into_iter()
        .map(|(version_hash, info)| {
            let (
                cells_count,
                live_cells_count,
                capacity_sum,
                live_capacity_sum,
                used_capacity_sum,
                live_used_capacity_sum,
            ) = version_totals(&info);
            let capacity_sum = capacity_sum as u128;
            let live_capacity_sum = live_capacity_sum as u128;
            let used_capacity_sum = used_capacity_sum as u128;
            let live_used_capacity_sum = live_used_capacity_sum as u128;

            total_cells += cells_count;
            total_live += live_cells_count;
            total_cap += capacity_sum;
            total_live_cap += live_capacity_sum;
            total_used_cap += used_capacity_sum;
            total_live_used_cap += live_used_capacity_sum;

            DeploymentUsage {
                code_hash: format!("0x{}", hex::encode(&version_hash)),
                script_kind: version_script_kind(&info),
                cells_count,
                live_cells_count,
                capacity_sum: capacity_sum.to_string(),
                live_capacity_sum: live_capacity_sum.to_string(),
                used_capacity_sum: used_capacity_sum.to_string(),
                live_used_capacity_sum: live_used_capacity_sum.to_string(),
            }
        })
        .collect();
```

**After:**
```rust
    let all_script_infos = load_script_infos_cached(&state)?;

    let mut total_cells: i64 = 0;
    let mut total_live: i64 = 0;
    let mut total_cap: u128 = 0;
    let mut total_live_cap: u128 = 0;
    let mut total_used_cap: u128 = 0;
    let mut total_live_used_cap: u128 = 0;

    let by_deployment: Vec<DeploymentUsage> = matching
        .into_iter()
        .map(|(version_hash, info)| {
            let (
                cells_count,
                live_cells_count,
                capacity_sum,
                live_capacity_sum,
                used_capacity_sum,
                live_used_capacity_sum,
            ) = resolve_version_capacity(&info, None, &all_script_infos)?;
            // Safe: resolve_version_capacity validates non-negative values
            let capacity_sum = capacity_sum as u128;
            let live_capacity_sum = live_capacity_sum as u128;
            let used_capacity_sum = used_capacity_sum as u128;
            let live_used_capacity_sum = live_used_capacity_sum as u128;

            total_cells += cells_count;
            total_live += live_cells_count;
            total_cap += capacity_sum;
            total_live_cap += live_capacity_sum;
            total_used_cap += used_capacity_sum;
            total_live_used_cap += live_used_capacity_sum;

            Ok(DeploymentUsage {
                code_hash: format!("0x{}", hex::encode(&version_hash)),
                script_kind: version_script_kind(&info),
                cells_count,
                live_cells_count,
                capacity_sum: capacity_sum.to_string(),
                live_capacity_sum: live_capacity_sum.to_string(),
                used_capacity_sum: used_capacity_sum.to_string(),
                live_used_capacity_sum: live_used_capacity_sum.to_string(),
            })
        })
        .collect::<Result<Vec<_>, ApiRouteError>>()?;
```

Key changes:
- Closure now returns `Result<DeploymentUsage, ApiRouteError>` (was bare `DeploymentUsage`)
- `.collect()` becomes `.collect::<Result<Vec<_>, ApiRouteError>>()?`
- `version_totals(&info)` → `resolve_version_capacity(&info, None, &all_script_infos)?`
- `i128 as u128` casts are now safe because `resolve_version_capacity` validates all capacity values non-negative (including historical totals, not just live)

- [ ] **Step 2: Run all tests**

Run: `cargo test -p ckbadger-api -- --nocapture`
Expected: All tests PASS

- [ ] **Step 3: Run clippy**

Run: `cargo clippy -p ckbadger-api`
Expected: No new warnings

- [ ] **Step 4: Run full pre-commit check**

Run: `cargo check && cargo clippy`
Expected: PASS — all crates compile, no warnings

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/routes/scripts.rs
git commit -m "fix(api): use resolve_version_capacity in get_script_usage, fix unsafe casts"
```
