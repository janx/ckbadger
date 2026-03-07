# Append-Only History Indices Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Move `activities`, `addr_txs`, and `nft_collection_activities` to a reorg-safe append-only design that preserves orphaned history while keeping API responses canonical-only.

**Architecture:** Re-key all three history indexes so append-only uniqueness includes `tx_hash`, move `CF_ACTIVITIES` into the append-only RocksDB, and keep canonical truth in domain `tx_hash_map` / `tx_index`. Indexer write paths stop mutating history on reorg; API and repair paths scan append-only history and filter rows through domain canonical location checks.

**Tech Stack:** Rust, RocksDB, `bincode`, inline unit tests, `cargo test` for `ckbadger-store`, `ckbadger-indexer`, and `ckbadger-api`

---

### Task 1: Lock the new key contracts in store tests

**Files:**

- Modify: `crates/ckbadger-store/src/keys.rs`
- Test: `crates/ckbadger-store/src/keys.rs`

**Step 1: Write the failing tests**

Add tests for the new key shapes:

```rust
#[test]
fn test_encode_addr_tx_key_includes_tx_hash() {
    let key = encode_addr_tx_key(&[0x11; 32], 100, 3, &[0xAA; 32]);
    assert_eq!(key.len(), 76);
}

#[test]
fn test_encode_activity_key_includes_tx_hash() {
    let key = encode_activity_key(&[0x22; 32], 200, 7, &[0xBB; 32]);
    assert_eq!(key.len(), 76);
}

#[test]
fn test_encode_nft_collection_activity_key_includes_tx_hash() {
    let key = encode_nft_collection_activity_key(&[0x33; 32], 300, 9, &[0xCC; 32]);
    assert_eq!(key.len(), 76);
}
```

**Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p ckbadger-store test_encode_addr_tx_key_includes_tx_hash --lib
cargo test -p ckbadger-store test_encode_activity_key_includes_tx_hash --lib
cargo test -p ckbadger-store test_encode_nft_collection_activity_key_includes_tx_hash --lib
```

Expected: FAIL because the encoders still use the old 44-byte shape.

**Step 3: Write minimal implementation**

- Change the three encoders to append `tx_hash`.
- Add matching decode helpers for the new format.
- Keep descending sort semantics on `block_num` / `tx_idx`.

**Step 4: Run tests to verify they pass**

Run the same three commands.

Expected: PASS

**Step 5: Commit**

```bash
git add crates/ckbadger-store/src/keys.rs
git commit -m "refactor: rekey append-only history indexes with tx hash"
```

### Task 2: Move `CF_ACTIVITIES` into append-only store ownership

**Files:**

- Modify: `crates/ckbadger-store/src/store.rs`
- Test: `crates/ckbadger-store/src/store.rs`

**Step 1: Write the failing tests**

Add tests that express the new ownership:

```rust
#[test]
fn test_open_append_only_allows_activities_cf() {
    let dir = TempDir::new().unwrap();
    let store = CkbadgerStore::open_append_only(dir.path()).unwrap();
    let _ = store.cf_activities();
}

#[test]
fn test_open_domain_rejects_activities_cf() {
    let dir = TempDir::new().unwrap();
    let store = CkbadgerStore::open_domain(dir.path()).unwrap();
    let panicked = std::panic::catch_unwind(|| {
        let _ = store.cf_activities();
    })
    .is_err();
    assert!(panicked);
}
```

**Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p ckbadger-store test_open_append_only_allows_activities_cf --lib
cargo test -p ckbadger-store test_open_domain_rejects_activities_cf --lib
```

Expected: FAIL because `CF_ACTIVITIES` still belongs to `DOMAIN_CFS`.

**Step 3: Write minimal implementation**

- Move `CF_ACTIVITIES` from `DOMAIN_CFS` to `APPEND_CFS`.
- Update `append_cf_name_for_handle()` to recognize `cf_activities()`.
- Update any ownership assertions that assume only two append-only CFs.

**Step 4: Run tests to verify they pass**

Run the same two commands plus:

```bash
cargo test -p ckbadger-store test_open_append_only_restricts_domain_cfs --lib
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/ckbadger-store/src/store.rs
git commit -m "refactor: move activities cf to append-only store"
```

### Task 3: Update store readers for the new append-only key format

**Files:**

- Modify: `crates/ckbadger-store/src/activity_ops.rs`
- Modify: `crates/ckbadger-store/src/address_ops.rs`
- Modify: `crates/ckbadger-store/src/nft_ops.rs`
- Test: `crates/ckbadger-store/src/activity_ops.rs`
- Test: `crates/ckbadger-store/src/address_ops.rs`
- Test: `crates/ckbadger-store/src/nft_ops.rs`

**Step 1: Write the failing tests**

Add regression tests proving reorg-safe coexistence:

```rust
#[test]
fn test_list_activities_keeps_two_rows_same_position_different_tx_hash() { /* ... */ }

#[test]
fn test_list_addr_txs_recent_keeps_two_rows_same_position_different_tx_hash() { /* ... */ }

#[test]
fn test_list_nft_collection_activities_keeps_two_rows_same_position_different_tx_hash() { /* ... */ }
```

Each test should insert two rows with:

- same owner or collection,
- same `(block_num, tx_idx)`,
- different `tx_hash`,
- and assert both rows are returned in deterministic order.

**Step 2: Run tests to verify they fail**

Run the three focused tests under `ckbadger-store`.

Expected: FAIL because the old reader logic assumes 44-byte keys and one row per canonical position.

**Step 3: Write minimal implementation**

- Update readers to decode the new 76-byte keys.
- Keep cursor and prefix-scan behavior newest-first.
- Use `tx_hash` from the key or stored value consistently; do not introduce fallback chains.

**Step 4: Run tests to verify they pass**

Run the same three focused tests.

Expected: PASS

**Step 5: Commit**

```bash
git add crates/ckbadger-store/src/activity_ops.rs crates/ckbadger-store/src/address_ops.rs crates/ckbadger-store/src/nft_ops.rs
git commit -m "refactor: read append-only history indexes with tx-hash keys"
```

### Task 4: Write history indexes to append-only in the indexer

**Files:**

- Modify: `crates/indexer/src/sync/batch.rs`
- Modify: `crates/indexer/src/sync/undo.rs`
- Test: `crates/indexer/src/sync/undo.rs`

**Step 1: Write the failing tests**

Add or update tests so activity history is expected in append-only:

```rust
#[test]
fn test_put_activity_with_undo_log_targets_append_store() { /* ... */ }
```

Expected assertions:

- `CF_ACTIVITIES` is written through an append-only `StoreBatch`
- undo entry records `UndoLogStoreTarget::AppendOnly`

**Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p ckbadger-indexer test_put_activity_with_undo_log_targets_append_store --lib
```

Expected: FAIL because activity writes still target the domain store and domain undo target.

**Step 3: Write minimal implementation**

- In bulk sync, construct activity batches from `append_only_store`, not `store`.
- In live sync, construct activity batches from `self.append_only_store`.
- Change `put_activity_with_undo_log()` to record append-only undo entries.
- Keep canonical aggregates and mutable stats in the domain store.

**Step 4: Run tests to verify they pass**

Run the focused test plus:

```bash
cargo test -p ckbadger-indexer test_put_addr_tx_with_undo_log --lib
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/indexer/src/sync/batch.rs crates/indexer/src/sync/undo.rs
git commit -m "refactor: write activity history to append-only store"
```

### Task 5: Fix rollback and repair logic to preserve history and rebuild canonical aggregates

**Files:**

- Modify: `crates/ckbadger-store/src/reorg_ops.rs`
- Modify: `crates/ckbadger-store/src/address_ops.rs`
- Test: `crates/ckbadger-store/src/reorg_ops.rs`
- Test: `crates/indexer/tests/reorg_handling.rs`

**Step 1: Write the failing tests**

Add regression tests that prove:

- append-only `activities` survive rollback
- `addr_balance.txs_count` rebuild uses canonical filtering over append-only `addr_txs`
- `nft_collection_agg.activities_count` rebuild uses canonical filtering over append-only collection history

**Step 2: Run tests to verify they fail**

Run the focused rollback tests.

Expected: FAIL because existing rebuild paths either assume domain `activities` or do not handle the new key shape and canonical filtering.

**Step 3: Write minimal implementation**

- Rebuild mutable aggregates by scanning append-only history and validating each row against domain canonical tx location.
- Do not add delete/update paths to append-only history.
- Keep failure messages specific with tx hash, block, collection, or lock hash context.

**Step 4: Run tests to verify they pass**

Run the focused rollback tests.

Expected: PASS

**Step 5: Commit**

```bash
git add crates/ckbadger-store/src/reorg_ops.rs crates/ckbadger-store/src/address_ops.rs crates/indexer/tests/reorg_handling.rs
git commit -m "fix: rebuild canonical aggregates from append-only history"
```

### Task 6: Update API routes and replace unified-store tests for split-sensitive paths

**Files:**

- Modify: `crates/api/src/routes/activities.rs`
- Modify: `crates/api/src/routes/cells.rs`
- Modify: `crates/api/src/routes/assets.rs`
- Modify: `crates/api/src/routes/spore.rs`
- Modify: `crates/api/tests/api_integration.rs`

**Step 1: Write the failing tests**

Add or update API integration tests so they use real dual stores for split-sensitive endpoints:

- address activities
- address transactions
- NFT collection activities
- spore cluster activities

Each test should:

- write canonical tx metadata into the domain store,
- write history rows into the append-only store,
- include at least one orphaned append-only row,
- assert only canonical rows are returned.

**Step 2: Run tests to verify they fail**

Run the focused `ckbadger-api` tests.

Expected: FAIL because many existing tests still use `open_test_unified()` and do not exercise the split boundary correctly.

**Step 3: Write minimal implementation**

- Update route helpers to decode the new key shapes where needed.
- Keep append-only as the history source and domain as the canonical filter source.
- Replace split-sensitive test helpers with real `open_domain()` + `open_append_only()` pairs.

**Step 4: Run tests to verify they pass**

Run the focused API tests.

Expected: PASS

**Step 5: Commit**

```bash
git add crates/api/src/routes/activities.rs crates/api/src/routes/cells.rs crates/api/src/routes/assets.rs crates/api/src/routes/spore.rs crates/api/tests/api_integration.rs
git commit -m "test: exercise split-store history routes with real dual stores"
```

### Task 7: Align docs with the approved store semantics

**Files:**

- Modify: `docs/prompts/ACTIVITY_SYSTEM.md`
- Modify: `docs/prompts/REORG_HANDLING.md`
- Modify: `docs/STORE_SCHEMA.md`
- Modify: `docs/prompts/INFORMATION_DESIGN.md`

**Step 1: Write the failing documentation checks**

Use `rg` checks to capture stale wording:

```bash
rg -n "delete.*activities|target_store=AppendOnly.*activities|CF_ACTIVITIES" docs crates -g '!target'
```

Expected stale findings:

- rollback docs still describe deleting `activities`
- schema docs still place `activities` in domain
- information docs do not clarify layer vs store responsibility

**Step 2: Update the docs**

- `ACTIVITY_SYSTEM.md`: document append-only placement and tx-hash-extended key shape.
- `REORG_HANDLING.md`: document history preservation and canonical filtering.
- `STORE_SCHEMA.md`: move `activities` into append-only ownership.
- `INFORMATION_DESIGN.md`: clarify that domain knowledge can be stored in append-only when the record is immutable history.

**Step 3: Run the documentation checks**

Run the same `rg` command and manually inspect remaining hits.

Expected: only intentional references remain.

**Step 4: Commit**

```bash
git add docs/prompts/ACTIVITY_SYSTEM.md docs/prompts/REORG_HANDLING.md docs/STORE_SCHEMA.md docs/prompts/INFORMATION_DESIGN.md
git commit -m "docs: align history index semantics with append-only design"
```

### Task 8: Run full validation and capture any remaining gaps

**Files:**

- No code changes required unless validation exposes a defect

**Step 1: Run focused crate tests**

Run:

```bash
cargo test -p ckbadger-store
cargo test -p ckbadger-indexer
cargo test -p ckbadger-api
```

Expected: PASS

**Step 2: Run broader type checking**

Run:

```bash
cargo check
```

Expected: PASS

**Step 3: If any test fails**

- Stop.
- Fix the specific failure in a fresh, minimal follow-up commit.
- Re-run the affected crate test before continuing.

**Step 4: Final commit if needed**

If Task 8 required fixes:

```bash
git add <files-fixed-during-validation>
git commit -m "fix: address validation regressions in history index migration"
```
