# Sole Spores Sentinel Collection Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Group all clusterless Spores (non-did:ckb) into a sentinel "Sole Spores" collection so they get the same collection infrastructure (aggregates, holders, activities) as real clusters.

**Architecture:** Define `SOLE_SPORES_SENTINEL_COLLECTION` constant following the existing Identity sentinel pattern. Modify the indexer writer to assign this sentinel as `collection_id` for clusterless Spores and update aggregates/indexes. Modify batch.rs activity recording to include the sentinel. Modify the API to accept `"sole-spores"` as a URL alias and return hardcoded metadata. Requires DB rebuild.

**Tech Stack:** Rust (ckbadger-store types, indexer writer, batch.rs, API routes)

---

## Change Summary

There are 4 touch points (3 in Rust backend, 0 in frontend):

1. **Store types** — Add constant
2. **Indexer writer** — Assign sentinel `collection_id` for clusterless spores in insert + consume
3. **Batch.rs** — Record activities for sole spores (2 code paths: live sync + grouped blocks)
4. **API** — Accept `"sole-spores"` alias, return hardcoded metadata for sentinel

---

### Task 1: Add sentinel constant to store types

**Files:**

- Modify: `crates/ckbadger-store/src/types.rs`

**Step 1: Add the constant**

After the existing `DID_CKB_SENTINEL_COLLECTION` constant (line ~418), add:

```rust
/// Sentinel collection key for clusterless Spore NFTs (32 bytes).
pub const SOLE_SPORES_SENTINEL_COLLECTION: [u8; 32] = *b"sole_spores_collection__________";
```

**Step 2: Verify it compiles**

Run: `cargo check -p ckbadger-store`
Expected: success

**Step 3: Commit**

```
feat(store): add SOLE_SPORES_SENTINEL_COLLECTION sentinel constant
```

---

### Task 2: Indexer writer — assign sentinel for clusterless spores on insert

**Files:**

- Modify: `crates/indexer/src/db/writer/spore.rs`

**Context:** `insert_spore_cell()` (line ~447) currently stores `collection_id: new_cluster.clone()` which is `None` for clusterless spores. The cluster aggregate block (line ~643) only runs `if let Some(ref cluster_id) = new_cluster`. We need clusterless spores to use the sentinel.

**Step 1: Import the sentinel**

At top of file, change:

```rust
use ckbadger_store::types::DID_CKB_SENTINEL_COLLECTION;
```

to:

```rust
use ckbadger_store::types::{DID_CKB_SENTINEL_COLLECTION, SOLE_SPORES_SENTINEL_COLLECTION};
```

**Step 2: Replace `new_cluster` with `effective_cluster` after the did:ckb early return**

After line 556 (`let new_cluster = spore.cluster_id.clone();`), add:

```rust
let effective_cluster = new_cluster.or_else(|| Some(SOLE_SPORES_SENTINEL_COLLECTION.to_vec()));
```

Then replace all subsequent references to `new_cluster` within `insert_spore_cell` with `effective_cluster`:

- Line ~560: `if let Some(cluster_id) = new_cluster.as_ref()` → `effective_cluster.as_ref()`
- Line ~582: `collection_id: new_cluster.clone()` → `effective_cluster.clone()`
- Line ~607: `if old_cluster != new_cluster` → `if old_cluster != effective_cluster`
- Line ~643: `if let Some(ref cluster_id) = new_cluster` → `if let Some(ref cluster_id) = effective_cluster`

**Important:** The `cluster_description` lookup (lines 560-572) should still use `new_cluster` (the raw on-chain cluster_id) not `effective_cluster`, because the sentinel has no real ObjectEntry to look up:

```rust
let cluster_description = if let Some(cluster_id) = new_cluster.as_ref() {
    // ... existing lookup code unchanged
```

**Step 3: Handle consume_spore to return sentinel for clusterless spores**

In `consume_spore()` (line ~753), the current code returns `cluster_id` from `entry.collection_id.clone()` (line ~802). Since clusterless spores will now have `collection_id = Some(SOLE_SPORES_SENTINEL_COLLECTION)`, the consume path will naturally return the sentinel, which will drive activity recording. **No change needed here.**

**Step 4: Verify it compiles**

Run: `cargo check -p ckbadger-indexer`
Expected: success

**Step 5: Add test for sole spore insert**

In the existing test module at the bottom of `spore.rs`, add a test that verifies a clusterless spore gets the sentinel collection_id and updates the sentinel aggregate:

```rust
#[test]
fn test_insert_clusterless_spore_uses_sole_spores_sentinel() {
    use ckbadger_store::types::SOLE_SPORES_SENTINEL_COLLECTION;

    let store = CkbadgerStore::open_test_unified().unwrap();
    let writer = BatchWriter::new(store);
    let mut batch = StoreBatch::new(writer.store());
    let mut state = writer.new_spore_batch_state();

    let spore_id = [0x11u8; 32];
    let owner = [0x22u8; 32];
    let tx_hash = [0x33u8; 32];

    let spore = make_parsed_spore_no_cluster(&spore_id, &owner);
    writer
        .insert_spore_cell(&spore, &tx_hash, 0, 100, 100_000, &mut batch, &mut state)
        .unwrap();

    // Verify the spore was stored with the sentinel collection_id
    let cached = state.spores.get(&spore_id.to_vec()).unwrap().as_ref().unwrap();
    assert_eq!(
        cached.collection_id.as_deref(),
        Some(SOLE_SPORES_SENTINEL_COLLECTION.as_slice()),
        "clusterless spore must get SOLE_SPORES_SENTINEL_COLLECTION"
    );

    // Verify cluster aggregate was updated
    let agg = state
        .cluster_aggs
        .get(&SOLE_SPORES_SENTINEL_COLLECTION.to_vec())
        .expect("sentinel aggregate must exist");
    assert_eq!(agg.total_count, 1);
    assert_eq!(agg.live_count, 1);
    assert_eq!(agg.owner_count, 1);
}
```

Also add the test helper (if not already present):

```rust
fn make_parsed_spore_no_cluster(spore_id: &[u8; 32], owner: &[u8; 32]) -> ParsedSporeCell {
    ParsedSporeCell {
        spore_id: spore_id.to_vec(),
        type_script_hash: vec![0u8; 32],
        is_did: false,
        content_type: "image/png".to_string(),
        content: b"test".to_vec(),
        cluster_id: None,
        owner_lock_hash: owner.to_vec(),
        media_profile: Some(SporeMediaProfile::default()),
    }
}
```

**Step 6: Run test**

Run: `cargo test -p ckbadger-indexer test_insert_clusterless_spore_uses_sole_spores_sentinel`
Expected: PASS

**Step 7: Commit**

```
feat(indexer): assign SOLE_SPORES_SENTINEL_COLLECTION to clusterless spores
```

---

### Task 3: Batch.rs — record activities for sole spores

**Files:**

- Modify: `crates/indexer/src/sync/batch.rs`

**Context:** There are two identical code patterns (live sync ~line 2642, grouped blocks ~line 5582) that record collection activities for spore inserts:

```rust
} else if let Some(ref cid) = spore.cluster_id {
    object_activity_acc.record(cid.as_slice(), ...);
}
```

These skip clusterless spores. We need to record the sentinel instead.

**Step 1: Import the sentinel**

Near the top of batch.rs, add to the existing imports:

```rust
use ckbadger_store::types::SOLE_SPORES_SENTINEL_COLLECTION;
```

**Step 2: Modify live sync insert path (~line 2642)**

Change:

```rust
} else if let Some(ref cid) = spore.cluster_id {
    object_activity_acc.record(
        cid.as_slice(),
        ...
    );
}
```

To:

```rust
} else {
    let cid = spore.cluster_id.as_deref()
        .unwrap_or(&SOLE_SPORES_SENTINEL_COLLECTION);
    object_activity_acc.record(
        cid,
        &tx_data.hash,
        &spore.spore_id,
        &parsed.hash,
        parsed.number,
        checked_usize_to_i32(tx_idx, "tx_idx"),
        parsed.timestamp.timestamp_millis(),
        true,
    );
}
```

**Step 3: Modify grouped blocks insert path (~line 5582)**

Same pattern — change the `else if let Some(ref cid) = spore.cluster_id` to the same `else` block with `unwrap_or`:

```rust
} else {
    let cid = spore.cluster_id.as_deref()
        .unwrap_or(&SOLE_SPORES_SENTINEL_COLLECTION);
    object_activity_acc.record(
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

**Step 4: Consume paths — no change needed**

The consume paths (live ~line 2801, grouped ~line 5838) already use `consume_spore()` return value which will now return the sentinel for clusterless spores (since `collection_id` is set in Task 2). No changes needed.

**Step 5: Verify it compiles**

Run: `cargo check -p ckbadger-indexer`
Expected: success

**Step 6: Commit**

```
feat(indexer): record collection activities for sole spores
```

---

### Task 4: API — accept "sole-spores" alias and return hardcoded metadata

**Files:**

- Modify: `crates/api/src/routes/spore.rs`

**Step 1: Import the sentinel**

Add to imports at top of file:

```rust
use ckbadger_store::types::SOLE_SPORES_SENTINEL_COLLECTION;
```

**Step 2: Add `parse_cluster_id_param` helper**

Add a helper function near the existing `parse_fixed_len_hex` helper:

```rust
/// Parse a cluster_id URL parameter. Accepts "sole-spores" alias
/// or a 32-byte hex string.
fn parse_cluster_id_param(
    raw: &str,
) -> Result<Vec<u8>, (axum::http::StatusCode, axum::Json<ApiError>)> {
    if raw.eq_ignore_ascii_case("sole-spores") {
        return Ok(SOLE_SPORES_SENTINEL_COLLECTION.to_vec());
    }
    parse_fixed_len_hex(raw, 32, "Invalid cluster ID (expected 32-byte hex or 'sole-spores')")
}

fn is_sole_spores_sentinel(id: &[u8]) -> bool {
    id == SOLE_SPORES_SENTINEL_COLLECTION
}
```

**Step 3: Update `get_cluster` handler**

In `get_cluster` (line ~1435), replace the raw hex parsing:

```rust
let id = hex::decode(cluster_id.strip_prefix("0x").unwrap_or(&cluster_id))
    .map_err(|_| ApiError::bad_request("Invalid cluster ID"))?;
```

With:

```rust
let id = parse_cluster_id_param(&cluster_id)?;
```

Then modify the metadata resolution section. After `get_cluster_aggregate`, replace the block that reads name/description/owner from `cluster_entry` (lines ~1474-1482):

```rust
let (name, description, owner_lock_hash, created_at_block) = if is_sole_spores_sentinel(&id) {
    (
        Some("Sole Spores".to_string()),
        Some("Spores not belonging to any cluster".to_string()),
        None,
        0i64,
    )
} else {
    let name = cluster_entry.as_ref().and_then(|e| e.name.clone());
    let description = cluster_entry.as_ref().and_then(|e| e.description.clone());
    let owner_lock_hash = cluster_entry.as_ref().and_then(|e| e.owner_lock_hash.clone());
    let created_at_block = cluster_entry.as_ref().map(|e| e.created_at_block).unwrap_or(0);
    (name, description, owner_lock_hash, created_at_block)
};
```

Also update the not-found check (line ~1458):

```rust
if spores_count == 0 && cluster_entry.is_none() && !is_sole_spores_sentinel(&id) {
    return Err(ApiError::not_found("Cluster not found"));
}
```

**Step 4: Update other cluster endpoints to use `parse_cluster_id_param`**

Apply the same `parse_cluster_id_param` replacement to:

- `get_cluster_occupation_chart` — find the hex decode and replace
- `get_cluster_holders` — find the hex decode and replace
- `get_cluster_activities` — find the hex decode and replace
- `get_spores_by_cluster` — find the hex decode and replace

Search each handler for `hex::decode(cluster_id.strip_prefix("0x")` and replace with `parse_cluster_id_param(&cluster_id)?`.

**Step 5: Update `spore_to_response` to hide sentinel from cluster_id field**

In `spore_to_response` (line ~296), the `cluster_id` field should show `None` for sole spores (they have no real on-chain cluster):

```rust
cluster_id: entry
    .collection_id
    .as_ref()
    .filter(|c| !is_sole_spores_sentinel(c))
    .map(|c| format!("0x{}", hex::encode(c))),
```

**Step 6: Verify it compiles**

Run: `cargo check -p ckbadger-api`
Expected: success

**Step 7: Add test for URL alias parsing**

```rust
#[test]
fn test_parse_cluster_id_param_sole_spores_alias() {
    let result = parse_cluster_id_param("sole-spores").unwrap();
    assert_eq!(result, SOLE_SPORES_SENTINEL_COLLECTION.to_vec());

    let result = parse_cluster_id_param("Sole-Spores").unwrap();
    assert_eq!(result, SOLE_SPORES_SENTINEL_COLLECTION.to_vec());
}

#[test]
fn test_parse_cluster_id_param_hex() {
    let hex_id = "ab".repeat(32);
    let result = parse_cluster_id_param(&hex_id).unwrap();
    assert_eq!(result, vec![0xab; 32]);

    let hex_id_0x = format!("0x{}", "cd".repeat(32));
    let result = parse_cluster_id_param(&hex_id_0x).unwrap();
    assert_eq!(result, vec![0xcd; 32]);
}

#[test]
fn test_is_sole_spores_sentinel() {
    assert!(is_sole_spores_sentinel(&SOLE_SPORES_SENTINEL_COLLECTION));
    assert!(!is_sole_spores_sentinel(&[0xab; 32]));
}
```

**Step 8: Run tests**

Run: `cargo test -p ckbadger-api test_parse_cluster_id_param`
Run: `cargo test -p ckbadger-api test_is_sole_spores_sentinel`
Expected: PASS

**Step 9: Commit**

```
feat(api): support "sole-spores" alias and hardcoded metadata for sentinel cluster
```

---

### Task 5: Full verification

**Step 1: Run all Rust tests**

Run: `cargo test`
Expected: all pass

**Step 2: Run clippy**

Run: `cargo clippy`
Expected: no new warnings

**Step 3: Commit if any fixes needed from clippy**

---

## Post-implementation

After merging, delete RocksDB data directory and re-sync from genesis to populate the sentinel collection data.
