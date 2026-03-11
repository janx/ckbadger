# Sole Spore Collection Bugs — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix 5 bugs where the sole spore collection (clusterless spores grouped under a sentinel) has broken name display, missing search results, wrong tab label, empty activities, and empty capacity data.

**Architecture:** Bugs 1-2 are a single API display-name fix that cascades to search. Bug 3 is a frontend label change. Bugs 4-5 are indexer pipeline fixes where bulk sync and parser skip sole spores due to `if let Some(cluster_id)` guards that should use the sentinel fallback. After fixing 4-5, a DB rebuild (re-sync from genesis) is required.

**Tech Stack:** Rust (axum API, indexer pipeline), React/TypeScript frontend

---

### Task 1: Fix sole spore collection display name (Bugs 1 & 2)

**Files:**

- Modify: `crates/api/src/utils/assets.rs:1-4` (add import)
- Modify: `crates/api/src/utils/assets.rs:162-177` (add sentinel check)
- Test: `crates/api/src/utils/assets.rs` (existing test module, add new test)

**Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block at the bottom of `crates/api/src/utils/assets.rs` (after the existing tests, before the closing `}`):

```rust
#[test]
fn resolve_dob_name_returns_sole_spores_for_sentinel() {
    use ckbadger_store::types::SOLE_SPORES_SENTINEL_COLLECTION;
    let (_dir, store) = test_store();
    let resolved =
        resolve_dob_collection_name(&store, &SOLE_SPORES_SENTINEL_COLLECTION, None);
    assert_eq!(resolved.as_deref(), Some("[Sole Spores]"));
}

#[test]
fn resolve_dob_name_aggregate_name_overrides_sentinel() {
    use ckbadger_store::types::SOLE_SPORES_SENTINEL_COLLECTION;
    let (_dir, store) = test_store();
    let resolved = resolve_dob_collection_name(
        &store,
        &SOLE_SPORES_SENTINEL_COLLECTION,
        Some("Custom Name"),
    );
    assert_eq!(resolved.as_deref(), Some("Custom Name"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p ckbadger-api --lib resolve_dob_name_returns_sole_spores`
Expected: FAIL — returns `None` instead of `Some("[Sole Spores]")`

**Step 3: Implement the fix**

In `crates/api/src/utils/assets.rs`, add `SOLE_SPORES_SENTINEL_COLLECTION` to the import on line 2:

```rust
use ckbadger_store::types::{
    ObjectStandard, DID_CKB_SENTINEL_COLLECTION, DOTBIT_SENTINEL_COLLECTION,
    SOLE_SPORES_SENTINEL_COLLECTION,
};
```

Then modify `resolve_dob_collection_name` (line 162-177) to check the sentinel before the DB fallback:

```rust
pub fn resolve_dob_collection_name(
    store: &CkbadgerStore,
    cluster_id: &[u8],
    aggregate_name: Option<&str>,
) -> Option<String> {
    if let Some(name) = non_empty_name(aggregate_name) {
        return Some(name);
    }

    if cluster_id == SOLE_SPORES_SENTINEL_COLLECTION {
        return Some("[Sole Spores]".to_string());
    }

    match store.get_spore(cluster_id) {
        Ok(Some(entry)) if entry.standard == ObjectStandard::SporeCluster => {
            non_empty_name(entry.name.as_deref())
        }
        _ => None,
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p ckbadger-api --lib resolve_dob_name`
Expected: All 5 `resolve_dob_name_*` tests PASS (3 existing + 2 new)

**Step 5: Commit**

```
feat(api): fix sole spore collection display name

The sentinel collection was missing a name override in
resolve_dob_collection_name(), causing it to show as
"Unnamed Collection" on the assets page and be invisible
to search. Returns "[Sole Spores]" for the sentinel.
```

---

### Task 2: Rename "NFTs" tab to "Objects" on collection detail page (Bug 3)

**Files:**

- Modify: `frontend/app/clusters/[clusterId]/client-page.tsx:558-577`

**Step 1: Change the tab trigger label**

In `frontend/app/clusters/[clusterId]/client-page.tsx`, change line 562-563 from:

```tsx
<TabsTrigger value="nfts">NFTs ({formatNumber(cluster.sporesCount)})</TabsTrigger>
```

to:

```tsx
<TabsTrigger value="nfts">Objects ({formatNumber(cluster.sporesCount)})</TabsTrigger>
```

**Step 2: Change the panel header text**

On line 572-576, change the ternary from:

```tsx
{
  activeCollectionTab === 'activities'
    ? 'Activities'
    : activeCollectionTab === 'holders'
      ? 'Holders'
      : 'NFTs';
}
```

to:

```tsx
{
  activeCollectionTab === 'activities'
    ? 'Activities'
    : activeCollectionTab === 'holders'
      ? 'Holders'
      : 'Objects';
}
```

**Step 3: Run frontend type-check and lint**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: No errors

**Step 4: Commit**

```
feat(frontend): rename NFTs tab to Objects on collection detail page
```

---

### Task 3: Fix bulk sync activity recording for sole spores (Bug 4)

**Files:**

- Modify: `crates/indexer/src/sync/batch.rs:17-20` (add sentinel import)
- Modify: `crates/indexer/src/sync/batch.rs:2210-2221` (fix activity recording)

**Step 1: Add SOLE_SPORES_SENTINEL_COLLECTION to imports**

In `crates/indexer/src/sync/batch.rs`, line 17-20, change:

```rust
use ckbadger_store::types::{
    DailyActivityStats, DaoDailySnapshot, IdentityCollectionAggregate, LiveCellInfo,
    ObjectTypeIndex, SporeTypeIndex,
};
```

to:

```rust
use ckbadger_store::types::{
    DailyActivityStats, DaoDailySnapshot, IdentityCollectionAggregate, LiveCellInfo,
    ObjectTypeIndex, SOLE_SPORES_SENTINEL_COLLECTION, SporeTypeIndex,
};
```

**Step 2: Fix the bulk sync activity recording guard**

In `crates/indexer/src/sync/batch.rs`, change lines 2210-2221 from:

```rust
                                    } else if let Some(ref cid) = spore.cluster_id {
                                        spore_activity_acc.record(
                                            cid.as_slice(),
                                            &tx_data.hash,
                                            &spore.spore_id,
                                            &parsed.hash,
                                            parsed.number,
                                            checked_usize_to_i32(tx_idx, "tx_idx"),
                                            ts_ms,
                                            true,
                                        );
                                    }
```

to:

```rust
                                    } else {
                                        let cid = spore
                                            .cluster_id
                                            .as_deref()
                                            .unwrap_or(&SOLE_SPORES_SENTINEL_COLLECTION);
                                        spore_activity_acc.record(
                                            cid,
                                            &tx_data.hash,
                                            &spore.spore_id,
                                            &parsed.hash,
                                            parsed.number,
                                            checked_usize_to_i32(tx_idx, "tx_idx"),
                                            ts_ms,
                                            true,
                                        );
                                    }
```

This matches the live sync pattern at line 3556-3571.

**Step 3: Verify compilation**

Run: `cargo check -p ckbadger-indexer`
Expected: Compiles without errors

**Step 4: Commit**

```
fix(indexer): record sole spore activities during bulk sync

Bulk sync skipped activity recording for clusterless spores
because it checked `if let Some(ref cid) = spore.cluster_id`.
Changed to use SOLE_SPORES_SENTINEL_COLLECTION fallback,
matching the existing live sync behavior.

Requires DB rebuild.
```

---

### Task 4: Fix parser daily deltas for sole spores (Bug 5)

**Files:**

- Modify: `crates/indexer/src/sync/pipeline.rs:22` (add sentinel import)
- Modify: `crates/indexer/src/sync/pipeline.rs:1295-1301` (creation path)
- Modify: `crates/indexer/src/sync/pipeline.rs:1428-1435` (consumption path)

**Step 1: Add SOLE_SPORES_SENTINEL_COLLECTION to imports**

In `crates/indexer/src/sync/pipeline.rs`, change line 22 from:

```rust
use ckbadger_store::types::DOTBIT_SENTINEL_COLLECTION;
```

to:

```rust
use ckbadger_store::types::{DOTBIT_SENTINEL_COLLECTION, SOLE_SPORES_SENTINEL_COLLECTION};
```

**Step 2: Fix the creation path**

In `crates/indexer/src/sync/pipeline.rs`, change lines 1295-1301 from:

```rust
                                if let Some(cluster_id) = cluster_id {
                                    let cluster_daily = cluster_daily_changes
                                        .entry((cluster_id, date_yyyymmdd))
                                        .or_insert((0, 0));
                                    cluster_daily.0 += i128::from(cell.capacity);
                                    cluster_daily.1 += i128::from(cell_occupied);
                                }
```

to:

```rust
                                {
                                    let effective_cluster_id = cluster_id
                                        .unwrap_or_else(|| SOLE_SPORES_SENTINEL_COLLECTION.to_vec());
                                    let cluster_daily = cluster_daily_changes
                                        .entry((effective_cluster_id, date_yyyymmdd))
                                        .or_insert((0, 0));
                                    cluster_daily.0 += i128::from(cell.capacity);
                                    cluster_daily.1 += i128::from(cell_occupied);
                                }
```

**Step 3: Fix the consumption path**

In `crates/indexer/src/sync/pipeline.rs`, change lines 1428-1435 from:

```rust
                                            if let Some(cluster_id) = index.cluster_id {
                                                let cluster_daily = cluster_daily_changes
                                                    .entry((cluster_id, date_yyyymmdd))
                                                    .or_insert((0, 0));
                                                cluster_daily.0 -= i128::from(info.capacity);
                                                cluster_daily.1 -=
                                                    i128::from(info.occupied_capacity);
                                            }
```

to:

```rust
                                            {
                                                let effective_cluster_id = index
                                                    .cluster_id
                                                    .unwrap_or_else(|| SOLE_SPORES_SENTINEL_COLLECTION.to_vec());
                                                let cluster_daily = cluster_daily_changes
                                                    .entry((effective_cluster_id, date_yyyymmdd))
                                                    .or_insert((0, 0));
                                                cluster_daily.0 -= i128::from(info.capacity);
                                                cluster_daily.1 -=
                                                    i128::from(info.occupied_capacity);
                                            }
```

**Step 4: Verify compilation**

Run: `cargo check -p ckbadger-indexer`
Expected: Compiles without errors

**Step 5: Commit**

```
fix(indexer): include sole spores in cluster daily capacity deltas

The parser skipped cluster_daily_changes for clusterless spores
(cluster_id = None), causing empty capacity/occupation data on
the sole spore collection page. Now uses sentinel as fallback
for both creation and consumption paths.

Requires DB rebuild.
```

---

### Task 5: Run full pre-commit checks

**Step 1: Rust checks**

Run: `cargo check && cargo clippy`
Expected: No errors or warnings

**Step 2: Rust tests**

Run: `cargo test --lib`
Expected: All tests pass

**Step 3: Frontend checks**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: No errors

**Step 4: Frontend tests**

Run: `cd frontend && npx vitest run`
Expected: All tests pass

---

## Post-implementation

After all fixes are deployed:

1. Delete RocksDB data directory
2. Re-sync from genesis (`ckbadger run`)
3. Verify sole spore collection page shows: correct name, populated activities, capacity data
4. Verify assets page objects tab shows "[Sole Spores]" instead of "Unnamed Collection"
5. Verify search for "sole" returns the sole spore collection
