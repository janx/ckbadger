# Remove timestamp from ActivityTxEnvelope - Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix infinite sync loop caused by reorg replay writing different timestamps to append-only activity envelopes.

**Architecture:** Remove `timestamp` from `ActivityTxEnvelope` (append-only storage), keep it in `ActivityEntry` (read-time struct). Derive timestamp from block headers at API read time. This makes the envelope purely tx-content-derived, ensuring idempotent append-only writes across reorg replays.

**Tech Stack:** Rust, RocksDB (bincode serialization), Axum API

**Design doc:** `docs/plans/2026-03-08-activity-envelope-timestamp-design.md`

---

### Task 1: Remove timestamp from ActivityTxEnvelope struct

**Files:**

- Modify: `crates/ckbadger-store/src/types.rs:936-944`

**Step 1: Remove timestamp field from ActivityTxEnvelope**

```rust
// crates/ckbadger-store/src/types.rs — lines 935-944
// BEFORE:
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActivityTxEnvelope {
    pub tx_hash: Vec<u8>,
    pub block_number: i64,
    pub tx_index: i32,
    pub timestamp: i64,
    pub is_cellbase: bool,
    pub participants: Vec<Vec<u8>>,
    pub owner_views: Vec<OwnerActivityViewStored>,
}

// AFTER:
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActivityTxEnvelope {
    pub tx_hash: Vec<u8>,
    pub block_number: i64,
    pub tx_index: i32,
    pub is_cellbase: bool,
    pub participants: Vec<Vec<u8>>,
    pub owner_views: Vec<OwnerActivityViewStored>,
}
```

**Step 2: Run `cargo check` to find all compile errors**

Run: `cargo check 2>&1 | head -80`
Expected: Multiple errors about missing `timestamp` field in ActivityTxEnvelope constructors.

**Step 3: Commit**

```bash
git add crates/ckbadger-store/src/types.rs
git commit -m "refactor: remove timestamp from ActivityTxEnvelope struct"
```

---

### Task 2: Fix activity_ops read path — set timestamp to 0 placeholder

**Files:**

- Modify: `crates/ckbadger-store/src/activity_ops.rs:157-167`

**Step 1: Change timestamp source to 0 placeholder**

```rust
// crates/ckbadger-store/src/activity_ops.rs — line 161
// BEFORE:
        timestamp: envelope.timestamp,
// AFTER:
        timestamp: 0,
```

**Step 2: Run `cargo check -p ckbadger-store`**

Expected: ckbadger-store compiles. Remaining errors in other crates.

**Step 3: Commit**

```bash
git add crates/ckbadger-store/src/activity_ops.rs
git commit -m "refactor: use placeholder timestamp in activity entry reconstruction"
```

---

### Task 3: Fix normalize_activities_for_storage — stop writing timestamp to envelope

**Files:**

- Modify: `crates/indexer/src/db/writer/activities.rs:126-208`

**Step 1: Remove timestamp from consistency check and envelope construction**

```rust
// crates/indexer/src/db/writer/activities.rs — lines 144-158
// Remove the timestamp comparison from the consistency check.
// BEFORE:
        for (_, entry) in &group {
            if entry.block_number != block_number
                || entry.tx_index != tx_index
                || entry.tx_hash != tx_hash
                || entry.timestamp != first.timestamp
                || entry.is_cellbase != first.is_cellbase
            {
// AFTER:
        for (_, entry) in &group {
            if entry.block_number != block_number
                || entry.tx_index != tx_index
                || entry.tx_hash != tx_hash
                || entry.is_cellbase != first.is_cellbase
            {
```

```rust
// crates/indexer/src/db/writer/activities.rs — lines 194-202
// Remove timestamp from envelope construction.
// BEFORE:
            envelope: ActivityTxEnvelope {
                tx_hash,
                block_number,
                tx_index,
                timestamp: first.timestamp,
                is_cellbase: first.is_cellbase,
                participants,
                owner_views,
            },
// AFTER:
            envelope: ActivityTxEnvelope {
                tx_hash,
                block_number,
                tx_index,
                is_cellbase: first.is_cellbase,
                participants,
                owner_views,
            },
```

**Step 2: Run `cargo check -p ckbadger-indexer`**

Expected: Compiles. Remaining errors in test code only.

**Step 3: Commit**

```bash
git add crates/indexer/src/db/writer/activities.rs
git commit -m "refactor: stop writing timestamp to activity envelope"
```

---

### Task 4: Fix all test helpers that construct ActivityTxEnvelope

**Files:**

- Modify: `crates/ckbadger-store/src/batch.rs` — `sample_envelope()` and `put_normalized_activity()`
- Modify: `crates/ckbadger-store/src/activity_ops.rs` — test helper `put_normalized_activity()`
- Modify: `crates/api/tests/api_integration.rs` — `put_normalized_activity()`
- Modify: `crates/indexer/tests/reorg_handling.rs` — `put_normalized_activity()`
- Modify: `crates/indexer/src/sync/undo.rs` — test envelope construction

In each file, remove the `timestamp: ...` line from every `ActivityTxEnvelope { ... }` construction.

**Step 1: Fix batch.rs test helpers**

```rust
// crates/ckbadger-store/src/batch.rs — sample_envelope() ~line 1060
// Remove: timestamp: 1_700_000_000,

// crates/ckbadger-store/src/batch.rs — put_normalized_activity() ~line 1081
// Remove: timestamp: entry.timestamp,
```

**Step 2: Fix activity_ops.rs test helper**

```rust
// crates/ckbadger-store/src/activity_ops.rs — put_normalized_activity() ~line 239
// Remove: timestamp: entry.timestamp,
```

**Step 3: Fix api_integration.rs test helper**

```rust
// crates/api/tests/api_integration.rs — put_normalized_activity() ~line 64
// Remove: timestamp: entry.timestamp,
```

**Step 4: Fix reorg_handling.rs test helper**

```rust
// crates/indexer/tests/reorg_handling.rs — put_normalized_activity() ~line 109
// Remove: timestamp: entry.timestamp,
```

**Step 5: Fix undo.rs test envelope**

```rust
// crates/indexer/src/sync/undo.rs — test NormalizedActivityTx ~line 456
// Remove: timestamp: 1_700_000_000,
```

**Step 6: Run `cargo check`**

Expected: Clean compile across all crates.

**Step 7: Run `cargo test --lib`**

Expected: All unit tests pass.

**Step 8: Commit**

```bash
git add -A
git commit -m "test: remove timestamp from all ActivityTxEnvelope test constructions"
```

---

### Task 5: Fill timestamps from block headers in API activity endpoint

**Files:**

- Modify: `crates/api/src/routes/activities.rs:240-309`

**Step 1: Add timestamp fill logic after loading activity page**

Replace the direct mapping with a two-phase approach: collect unique block numbers, batch-lookup headers, then map with real timestamps.

```rust
// crates/api/src/routes/activities.rs — in get_address_activities, after line 272
// BEFORE (lines 281-302):
    let activities: Vec<ActivityResponse> = page
        .into_iter()
        .map(|(_, _, entry)| ActivityResponse {
            tx_hash: format!("0x{}", hex::encode(&entry.tx_hash)),
            block_number: entry.block_number,
            tx_index: entry.tx_index,
            timestamp: entry.timestamp.to_string(),
            ...
        })
        .collect();

// AFTER:
    // Collect unique block numbers and look up timestamps from block headers
    let unique_blocks: Vec<i64> = page
        .iter()
        .map(|(block_num, _, _)| *block_num)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut block_timestamps: std::collections::HashMap<i64, i64> =
        std::collections::HashMap::new();
    for block_num in unique_blocks {
        if let Some(header) = state
            .store
            .get_block_header(block_num)
            .map_err(|e| ApiError::internal(e.to_string()))?
        {
            block_timestamps.insert(block_num, header.timestamp);
        }
    }

    let activities: Vec<ActivityResponse> = page
        .into_iter()
        .map(|(block_num, _, entry)| {
            let timestamp = block_timestamps
                .get(&block_num)
                .copied()
                .unwrap_or(0);
            ActivityResponse {
                tx_hash: format!("0x{}", hex::encode(&entry.tx_hash)),
                block_number: entry.block_number,
                tx_index: entry.tx_index,
                timestamp: timestamp.to_string(),
                ckb_delta: entry.ckb_delta.to_string(),
                occupied_delta: entry.occupied_delta.to_string(),
                is_cellbase: entry.is_cellbase,
                asset_changes: entry
                    .asset_changes
                    .iter()
                    .map(convert_asset_change)
                    .collect(),
                peers: entry
                    .peers
                    .iter()
                    .map(|h| format!("0x{}", hex::encode(h)))
                    .collect(),
            }
        })
        .collect();
```

**Step 2: Run `cargo check -p ckbadger-api`**

Expected: Compiles.

**Step 3: Run `cargo test -p ckbadger-api`**

Expected: Tests pass (API integration tests will get timestamp=0 since test domain stores don't have block headers, which is acceptable).

**Step 4: Commit**

```bash
git add crates/api/src/routes/activities.rs
git commit -m "feat: derive activity timestamps from block headers at API read time"
```

---

### Task 6: Run full test suite and verify

**Step 1: Run full Rust tests**

Run: `cargo test`
Expected: All tests pass.

**Step 2: Run clippy**

Run: `cargo clippy`
Expected: No new warnings.

**Step 3: Run frontend checks**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: Pass (no frontend changes).

**Step 4: Commit any fixes if needed**

---

### Task 7: Add regression test for reorg idempotent activity writes

**Files:**

- Modify: `crates/ckbadger-store/src/batch.rs` — add test in `#[cfg(test)]` module

**Step 1: Write test that simulates the reorg scenario**

Add a test that writes an activity envelope, then writes the same key with same tx content but verifies idempotent skip works (i.e., the envelope is truly deterministic now that timestamp is removed).

```rust
#[test]
fn test_activity_envelope_idempotent_write_after_reorg() {
    let dir = tempfile::tempdir().unwrap();
    let store = CkbadgerStore::open_append_only(dir.path()).unwrap();

    let envelope = ActivityTxEnvelope {
        tx_hash: vec![0x55; 32],
        block_number: 100,
        tx_index: 0,
        is_cellbase: true,
        participants: vec![vec![0x66; 32]],
        owner_views: vec![OwnerActivityViewStored {
            ckb_delta: 500_00000000,
            occupied_delta: 61_00000000,
            asset_changes: vec![],
        }],
    };

    // First write (initial sync)
    let mut batch1 = StoreBatch::new(&store);
    batch1.put_activity_tx_envelope(100, 0, &[0x55; 32], &envelope);
    batch1.commit().unwrap();

    // Second write (after reorg replay — same tx content, same envelope)
    let mut batch2 = StoreBatch::new(&store);
    batch2.put_activity_tx_envelope(100, 0, &[0x55; 32], &envelope);
    // This should succeed via idempotent skip (same key + same value)
    batch2.commit().unwrap();
}
```

**Step 2: Run the test**

Run: `cargo test -p ckbadger-store test_activity_envelope_idempotent_write_after_reorg`
Expected: PASS

**Step 3: Commit**

```bash
git add crates/ckbadger-store/src/batch.rs
git commit -m "test: add regression test for idempotent activity envelope writes after reorg"
```
