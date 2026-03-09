# Inline Rollback for Derived CFs — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace three full-table `rebuild_*` functions (addr_balance, script_info, token_state) with inline delta accumulation during rollback cell/token_transfer stages, eliminating multi-minute startup stalls.

**Architecture:** During existing cell rollback stages, accumulate balance/script/token deltas in HashMaps. After stage 8 (token_transfer deletion), apply all deltas to derived CFs in the same WriteBatch. Then remove the three rebuild functions and all call sites.

**Tech Stack:** Rust, RocksDB WriteBatch, bincode serialization, existing `CkbadgerStore` CF accessors

**Design doc:** `docs/plans/2026-03-09-inline-rollback-derived-cfs-design.md`

---

### Task 1: Add `accumulate_cell_deltas` helper function

**Files:**

- Modify: `crates/ckbadger-store/src/reorg_ops.rs` (insert after `put_cell_index_entries` ~line 179)

**Step 1: Add the helper function**

Insert after `put_cell_index_entries` (around line 179), before `load_tx_contexts_from_undo_log`:

```rust
/// Accumulate derived-CF deltas for a cell changing live state during rollback.
/// `sign` is -1 when removing from live (cell created after rollback_to),
/// +1 when restoring to live (cell consumed after rollback_to).
fn accumulate_cell_deltas(
    cell: &LiveCellInfo,
    sign: i128,
    addr_deltas: &mut HashMap<Vec<u8>, (i128, i128, i32)>,
    script_deltas: &mut HashMap<(Vec<u8>, bool), (i64, i128, i128)>,
    token_holder_deltas: &mut HashMap<(Vec<u8>, Vec<u8>), i128>,
) {
    let cap = cell.capacity as i128 * sign;
    let occ = cell.occupied_capacity as i128 * sign;
    let live_d = sign as i32;

    // addr_balance: (balance_delta, occupied_delta, live_cells_delta)
    let e = addr_deltas
        .entry(cell.lock_script_hash.clone())
        .or_insert((0, 0, 0));
    e.0 += cap;
    e.1 += occ;
    e.2 += live_d;

    // script_info — lock side: (live_cells_delta, live_cap_delta, live_occupied_delta)
    let e = script_deltas
        .entry((cell.lock_code_hash.clone(), false))
        .or_insert((0, 0, 0));
    e.0 += live_d as i64;
    e.1 += cap;
    e.2 += occ;

    // script_info — type side (if present)
    if let Some(ref type_code_hash) = cell.type_code_hash {
        let e = script_deltas
            .entry((type_code_hash.clone(), true))
            .or_insert((0, 0, 0));
        e.0 += live_d as i64;
        e.1 += cap;
        e.2 += occ;
    }

    // token_holder (UDT cells with type_script)
    if let (Some(ref type_script_hash), Some(udt_amount)) =
        (&cell.type_script_hash, cell.udt_amount)
    {
        if udt_amount > 0 {
            *token_holder_deltas
                .entry((type_script_hash.clone(), cell.lock_script_hash.clone()))
                .or_insert(0) += udt_amount as i128 * sign;
        }
    }
}
```

**Step 2: Verify it compiles**

Run: `cargo check -p ckbadger-store 2>&1 | head -20`
Expected: compiles (function is unused for now — allow dead_code warning)

**Step 3: Commit**

```bash
git add crates/ckbadger-store/src/reorg_ops.rs
git commit -m "refactor(store): add accumulate_cell_deltas helper for inline rollback"
```

---

### Task 2: Initialize delta maps and wire accumulation into cell rollback

**Files:**

- Modify: `crates/ckbadger-store/src/reorg_ops.rs`
  - Before cell stages (~line 629): declare 3 delta HashMaps
  - Fallback A — cell removal (~line 674-678): add accumulate call with sign=-1
  - Fallback B — cell restoration (~line 720-724): add accumulate call with sign=+1
  - Undo-log — cell removal (~line 762-770): add accumulate call with sign=-1
  - Undo-log — cell restoration (~line 823-832): add accumulate call with sign=+1

**Step 1: Add delta map declarations before the cell stages**

Before the `if !use_tx_context {` block (line 632), insert:

```rust
        // Delta accumulators for derived CFs, populated during cell rollback.
        // addr_deltas: lock_hash → (balance_delta, occupied_delta, live_cells_delta)
        let mut addr_balance_deltas: HashMap<Vec<u8>, (i128, i128, i32)> = HashMap::new();
        // script_deltas: (code_hash, is_type) → (live_cells_delta, live_cap_delta, live_occ_delta)
        let mut script_info_deltas: HashMap<(Vec<u8>, bool), (i64, i128, i128)> = HashMap::new();
        // token_holder_deltas: (type_hash, lock_hash) → balance_delta
        let mut token_holder_deltas: HashMap<(Vec<u8>, Vec<u8>), i128> = HashMap::new();
```

**Step 2: Wire accumulation in fallback path A (cell removal)**

After `cells_removed += 1;` (line 677), add:

```rust
                    accumulate_cell_deltas(
                        &info,
                        -1,
                        &mut addr_balance_deltas,
                        &mut script_info_deltas,
                        &mut token_holder_deltas,
                    );
```

**Step 3: Wire accumulation in fallback path B (cell restoration)**

After `cells_restored += 1;` (line 723), add:

```rust
                    accumulate_cell_deltas(
                        &info,
                        1,
                        &mut addr_balance_deltas,
                        &mut script_info_deltas,
                        &mut token_holder_deltas,
                    );
```

**Step 4: Wire accumulation in undo-log path — cell removal**

After `cells_removed += 1;` (~line 770), add:

```rust
                        accumulate_cell_deltas(
                            &info,
                            -1,
                            &mut addr_balance_deltas,
                            &mut script_info_deltas,
                            &mut token_holder_deltas,
                        );
```

**Step 5: Wire accumulation in undo-log path — cell restoration**

After `cells_restored += 1;` (~line 832), add:

```rust
                            accumulate_cell_deltas(
                                &consumed.cell,
                                1,
                                &mut addr_balance_deltas,
                                &mut script_info_deltas,
                                &mut token_holder_deltas,
                            );
```

**Step 6: Verify it compiles**

Run: `cargo check -p ckbadger-store 2>&1 | head -20`
Expected: compiles (delta maps populated but not yet consumed — allow unused warnings)

**Step 7: Commit**

```bash
git add crates/ckbadger-store/src/reorg_ops.rs
git commit -m "refactor(store): accumulate derived-CF deltas during cell rollback stages"
```

---

### Task 3: Add per-type_hash transfer count tracking in stage 8

**Files:**

- Modify: `crates/ckbadger-store/src/reorg_ops.rs` — stage 8 token_transfer deletion (~lines 1005-1026)

**Step 1: Add transfer_count_deltas map and populate during stage 8**

Before `let mut token_transfers_removed` (line 1007), add:

```rust
        // Per-type_hash count of deleted transfers, for TokenInfo.transfers_count update.
        let mut transfer_count_deltas: HashMap<Vec<u8>, i64> = HashMap::new();
```

Inside the `if block_num > rollback_to` block (after `token_transfers_removed += 1;` on line 1021), add:

```rust
                    let type_hash = key[0..32].to_vec();
                    *transfer_count_deltas.entry(type_hash).or_insert(0) += 1;
```

**Step 2: Verify it compiles**

Run: `cargo check -p ckbadger-store 2>&1 | head -20`
Expected: compiles (unused warning for transfer_count_deltas)

**Step 3: Commit**

```bash
git add crates/ckbadger-store/src/reorg_ops.rs
git commit -m "refactor(store): track per-type_hash transfer count deltas in rollback stage 8"
```

---

### Task 4: Apply all derived-CF deltas after stage 8

**Files:**

- Modify: `crates/ckbadger-store/src/reorg_ops.rs` — insert after stage 8, before stage 10 (spore/NFT repair, ~line 1028)

**Step 1: Add the delta application block**

Insert after `stage.finish(token_transfers_removed);` (line 1026) and before stage 10 comment:

```rust
        // 9. Apply derived-CF deltas (addr_balance, script_info, token_holders, token_info).
        let mut stage = RollbackStageProgress::new("apply_derived_cf_deltas");
        let mut addr_balances_updated = 0u64;
        let mut script_infos_updated = 0u64;
        let mut holders_updated = 0u64;
        let mut holders_removed = 0u64;
        let mut tokens_updated = 0u64;

        // 9a. addr_balance
        for (lock_hash, (balance_delta, occupied_delta, live_delta)) in &addr_balance_deltas {
            if *balance_delta == 0 && *occupied_delta == 0 && *live_delta == 0 {
                continue;
            }
            let mut ab = self.get_addr_balance(lock_hash)?.ok_or_else(|| {
                anyhow::anyhow!(
                    "missing addr_balance during rollback delta application: lock_hash=0x{}",
                    bytes_to_hex(lock_hash)
                )
            })?;
            ab.balance += balance_delta;
            ab.occupied_capacity += occupied_delta;
            ab.live_cells_count += live_delta;
            if ab.balance < 0 || ab.occupied_capacity < 0 || ab.live_cells_count < 0 {
                anyhow::bail!(
                    "addr_balance underflow during rollback: lock_hash=0x{}, balance={}, occupied={}, live_cells={}",
                    bytes_to_hex(lock_hash),
                    ab.balance,
                    ab.occupied_capacity,
                    ab.live_cells_count
                );
            }
            batch.put_cf(
                self.cf_addr_balance(),
                lock_hash,
                bincode::serialize(&ab).expect("serialize AddressBalance"),
            );
            addr_balances_updated += 1;
        }

        // 9b. script_info
        for ((code_hash, is_type), (live_delta, live_cap_delta, live_occ_delta)) in
            &script_info_deltas
        {
            if *live_delta == 0 && *live_cap_delta == 0 && *live_occ_delta == 0 {
                continue;
            }
            let mut si = self.get_script_info(code_hash)?.ok_or_else(|| {
                anyhow::anyhow!(
                    "missing script_info during rollback delta application: code_hash=0x{}, is_type={}",
                    bytes_to_hex(code_hash),
                    is_type
                )
            })?;
            if *is_type {
                si.type_live_cells_count += live_delta;
                si.type_live_capacity_sum += live_cap_delta;
                si.type_live_occupied_capacity_sum += live_occ_delta;
                if si.type_live_cells_count < 0
                    || si.type_live_capacity_sum < 0
                    || si.type_live_occupied_capacity_sum < 0
                {
                    anyhow::bail!(
                        "script_info type underflow during rollback: code_hash=0x{}, live={}, cap={}, occ={}",
                        bytes_to_hex(code_hash),
                        si.type_live_cells_count,
                        si.type_live_capacity_sum,
                        si.type_live_occupied_capacity_sum
                    );
                }
            } else {
                si.lock_live_cells_count += live_delta;
                si.lock_live_capacity_sum += live_cap_delta;
                si.lock_live_occupied_capacity_sum += live_occ_delta;
                if si.lock_live_cells_count < 0
                    || si.lock_live_capacity_sum < 0
                    || si.lock_live_occupied_capacity_sum < 0
                {
                    anyhow::bail!(
                        "script_info lock underflow during rollback: code_hash=0x{}, live={}, cap={}, occ={}",
                        bytes_to_hex(code_hash),
                        si.lock_live_cells_count,
                        si.lock_live_capacity_sum,
                        si.lock_live_occupied_capacity_sum
                    );
                }
            }
            batch.put_cf(
                self.cf_script_info(),
                code_hash,
                bincode::serialize(&si).expect("serialize ScriptInfo"),
            );
            script_infos_updated += 1;
        }

        // 9c. token_holders — apply balance deltas, track per-type_hash holder count changes
        let mut type_hash_holder_changes: HashMap<Vec<u8>, (i128, i64)> = HashMap::new();
        for ((type_hash, lock_hash), balance_delta) in &token_holder_deltas {
            if *balance_delta == 0 {
                continue;
            }
            let current =
                self.get_token_holder_balance(type_hash, lock_hash)?
                    .unwrap_or(0);
            let new_balance = current + balance_delta;
            let entry = type_hash_holder_changes
                .entry(type_hash.clone())
                .or_insert((0, 0));
            entry.0 += balance_delta; // total_supply delta

            if new_balance < 0 {
                anyhow::bail!(
                    "token_holder underflow during rollback: type=0x{}, lock=0x{}, current={}, delta={}",
                    bytes_to_hex(type_hash),
                    bytes_to_hex(lock_hash),
                    current,
                    balance_delta
                );
            } else if new_balance == 0 {
                let key = keys::encode_token_holder_key(type_hash, lock_hash);
                batch.delete_cf(self.cf_token_holders(), &key);
                if current > 0 {
                    entry.1 -= 1; // lost a holder
                }
                holders_removed += 1;
            } else {
                let key = keys::encode_token_holder_key(type_hash, lock_hash);
                batch.put_cf(
                    self.cf_token_holders(),
                    &key,
                    new_balance.to_le_bytes(),
                );
                if current == 0 {
                    entry.1 += 1; // gained a holder
                }
                holders_updated += 1;
            }
        }

        // 9d. token_info — merge holder changes and transfer count deltas
        let mut all_type_hashes: HashSet<Vec<u8>> =
            type_hash_holder_changes.keys().cloned().collect();
        all_type_hashes.extend(transfer_count_deltas.keys().cloned());
        for type_hash in &all_type_hashes {
            let (supply_delta, holders_delta) =
                type_hash_holder_changes.get(type_hash).copied().unwrap_or((0, 0));
            let transfers_removed =
                transfer_count_deltas.get(type_hash).copied().unwrap_or(0);
            if supply_delta == 0 && holders_delta == 0 && transfers_removed == 0 {
                continue;
            }
            if let Some(mut ti) = self.get_token(type_hash)? {
                ti.holders_count += holders_delta;
                if let Some(ref mut ts) = ti.total_supply {
                    *ts += supply_delta;
                }
                ti.transfers_count -= transfers_removed;
                batch.put_cf(
                    self.cf_tokens(),
                    type_hash.as_slice(),
                    bincode::serialize(&ti).expect("serialize TokenInfo"),
                );
                // Also update CF_STATS_TOKEN total transfers count
                if transfers_removed != 0 {
                    let current_count =
                        self.get_token_transfers_count(type_hash)?;
                    let new_count = current_count - transfers_removed;
                    let stats_key = keys::encode_token_transfers_key(type_hash);
                    batch.put_cf(
                        self.cf_stats_token(),
                        &stats_key,
                        new_count.to_le_bytes(),
                    );
                }
                tokens_updated += 1;
            }
        }

        info!(
            addr_balances_updated,
            script_infos_updated,
            holders_updated,
            holders_removed,
            tokens_updated,
            "Rollback derived CF deltas applied"
        );
        stage.finish(
            addr_balances_updated
                + script_infos_updated
                + holders_updated
                + holders_removed
                + tokens_updated,
        );
```

**Step 2: Verify it compiles**

Run: `cargo check -p ckbadger-store 2>&1 | head -20`
Expected: compiles clean (all delta maps are now consumed)

**Step 3: Run existing tests**

Run: `cargo test -p ckbadger-store -- rollback 2>&1 | tail -20`
Expected: all rollback tests pass (inline deltas + rebuilds both running)

**Step 4: Commit**

```bash
git add crates/ckbadger-store/src/reorg_ops.rs
git commit -m "feat(store): apply derived-CF deltas inline during rollback"
```

---

### Task 5: Add integration test for inline rollback of derived CFs

**Files:**

- Modify: `crates/indexer/tests/reorg_handling.rs` — add new test

**Step 1: Add test helpers and test function**

Add at the end of the file (before any closing braces if applicable):

```rust
/// Create a cell with type script and UDT amount for token testing.
fn make_udt_cell(
    block_num: i64,
    lock_hash: &[u8],
    type_hash: &[u8],
    type_code_hash: &[u8],
    udt_amount: u128,
) -> LiveCellInfo {
    LiveCellInfo {
        capacity: 14_200_000_000, // 142 CKB (typical UDT cell)
        created_at_block: block_num,
        lock_script_hash: lock_hash.to_vec(),
        lock_code_hash: vec![0xAA; 32],
        lock_hash_type: 1,
        lock_args: vec![0xBB; 20],
        type_script_hash: Some(type_hash.to_vec()),
        type_code_hash: Some(type_code_hash.to_vec()),
        type_args: None,
        data_size: 16,
        occupied_capacity: 14_200_000_000,
        udt_amount: Some(udt_amount),
    }
}

#[test]
fn test_rollback_updates_derived_cfs_inline() {
    use ckbadger_store::types::{AddressBalance, ScriptInfo, TokenInfo};

    let (store, append) = setup_split_stores();
    let lock_hash = [1u8; 32];
    let lock_code_hash = [0xAAu8; 32];
    let type_hash = [0xCCu8; 32];
    let type_code_hash = [0xDDu8; 32];

    // Insert 4 blocks with regular cells (each has capacity=10_000_000_000)
    for block_num in 1..=4 {
        insert_full_block(&store, block_num, &lock_hash);
    }

    // Insert UDT cells for blocks 3 and 4 (separate tx hashes)
    let udt_amount: u128 = 500_000_000;
    for block_num in 3..=4i64 {
        let mut udt_tx_hash = vec![0u8; 32];
        udt_tx_hash[0..8].copy_from_slice(&block_num.to_le_bytes());
        udt_tx_hash[8] = 0xFF; // distinguish from regular tx hashes
        let udt_cell = make_udt_cell(
            block_num,
            &lock_hash,
            &type_hash,
            &type_code_hash,
            udt_amount,
        );
        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&udt_tx_hash, 0, &udt_cell);
        batch.put_cell_by_lock(&lock_hash, block_num, &udt_tx_hash, 0);
        batch.commit().unwrap();
    }

    // Write initial derived CF state matching 4 regular cells + 2 UDT cells
    let reg_cap: i128 = 10_000_000_000;
    let udt_cap: i128 = 14_200_000_000;
    {
        let mut batch = StoreBatch::new(&store);
        batch.put_addr_balance(
            &lock_hash,
            &AddressBalance {
                balance: 4 * reg_cap + 2 * udt_cap,
                occupied_capacity: 2 * udt_cap, // only UDT cells have occupied
                live_cells_count: 6,
                total_cells_count: 6,
                txs_count: 4,
                first_seen_block: 1,
                first_seen_tx: vec![0; 32],
                last_activity_block: 4,
                last_activity_tx: vec![0; 32],
            },
        );
        batch.put_script_info(
            &lock_code_hash,
            &ScriptInfo {
                code_hash: lock_code_hash.to_vec(),
                hash_type: 1,
                name: None,
                category: None,
                website: None,
                description: None,
                cells_count: 0,
                capacity_used: 0,
                lock_cells_count: 6,
                lock_live_cells_count: 6,
                lock_capacity_sum: 4 * reg_cap + 2 * udt_cap,
                lock_live_capacity_sum: 4 * reg_cap + 2 * udt_cap,
                lock_occupied_capacity_sum: 2 * udt_cap,
                lock_live_occupied_capacity_sum: 2 * udt_cap,
                type_cells_count: 0,
                type_live_cells_count: 0,
                type_capacity_sum: 0,
                type_live_capacity_sum: 0,
                type_occupied_capacity_sum: 0,
                type_live_occupied_capacity_sum: 0,
                dep_type_hash: None,
                dep_data_hash: None,
                code_cell_tx_hash: None,
            },
        );
        batch.put_script_info(
            &type_code_hash,
            &ScriptInfo {
                code_hash: type_code_hash.to_vec(),
                hash_type: 1,
                name: None,
                category: None,
                website: None,
                description: None,
                cells_count: 0,
                capacity_used: 0,
                lock_cells_count: 0,
                lock_live_cells_count: 0,
                lock_capacity_sum: 0,
                lock_live_capacity_sum: 0,
                lock_occupied_capacity_sum: 0,
                lock_live_occupied_capacity_sum: 0,
                type_cells_count: 2,
                type_live_cells_count: 2,
                type_capacity_sum: 2 * udt_cap,
                type_live_capacity_sum: 2 * udt_cap,
                type_occupied_capacity_sum: 2 * udt_cap,
                type_live_occupied_capacity_sum: 2 * udt_cap,
                dep_type_hash: None,
                dep_data_hash: None,
                code_cell_tx_hash: None,
            },
        );
        batch.put_token_holder(&type_hash, &lock_hash, (2 * udt_amount) as i128);
        batch.put_token(
            &type_hash,
            &TokenInfo {
                type_code_hash: type_code_hash.to_vec(),
                hash_type: 1,
                type_args: vec![],
                standard: "xUDT".to_string(),
                name: Some("Test".to_string()),
                symbol: Some("TST".to_string()),
                decimals: Some(8),
                total_supply: Some((2 * udt_amount) as i128),
                max_supply: None,
                holders_count: 1,
                first_seen_block: 3,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        );
        batch.commit().unwrap();
    }

    // Rollback to block 2 — removes blocks 3,4 (2 regular cells + 2 UDT cells)
    let result = store
        .rollback_to_block_with_tx_index_store(2, Some(&append))
        .unwrap();
    assert_eq!(result.blocks_removed, 2);
    assert_eq!(result.cells_removed, 4); // 2 regular + 2 UDT

    // Verify addr_balance: 2 regular cells remain
    let ab = store.get_addr_balance(&lock_hash).unwrap().unwrap();
    assert_eq!(ab.live_cells_count, 2);
    assert_eq!(ab.balance, 2 * reg_cap);
    assert_eq!(ab.occupied_capacity, 0); // UDT cells gone
    // txs_count untouched (append-only)
    assert_eq!(ab.txs_count, 4);

    // Verify script_info for lock_code_hash: 2 regular cells remain
    let si = store.get_script_info(&lock_code_hash).unwrap().unwrap();
    assert_eq!(si.lock_live_cells_count, 2);
    assert_eq!(si.lock_live_capacity_sum, 2 * reg_cap);
    assert_eq!(si.lock_live_occupied_capacity_sum, 0);

    // Verify script_info for type_code_hash: 0 UDT cells remain
    let si_type = store.get_script_info(&type_code_hash).unwrap().unwrap();
    assert_eq!(si_type.type_live_cells_count, 0);
    assert_eq!(si_type.type_live_capacity_sum, 0);
    assert_eq!(si_type.type_live_occupied_capacity_sum, 0);

    // Verify token_holder: balance should be 0 (holder deleted)
    let holder = store
        .get_token_holder_balance(&type_hash, &lock_hash)
        .unwrap();
    assert_eq!(holder, None); // deleted because balance reached 0

    // Verify token_info: holders_count=0, total_supply=0
    let ti = store.get_token(&type_hash).unwrap().unwrap();
    assert_eq!(ti.holders_count, 0);
    assert_eq!(ti.total_supply, Some(0));
}
```

**Important**: This test references `ScriptInfo` fields that may have additional fields not shown. The implementer MUST read the actual `ScriptInfo` struct definition at `crates/ckbadger-store/src/types.rs` and include ALL fields with appropriate defaults in the test `ScriptInfo` construction. Use `..Default::default()` if the struct implements Default, otherwise set all remaining fields explicitly.

**Step 2: Run the test**

Run: `cargo test -p ckbadger-indexer --test reorg_handling test_rollback_updates_derived_cfs_inline -- --nocapture 2>&1 | tail -30`
Expected: PASS (both inline deltas and rebuild functions are active)

**Step 3: Commit**

```bash
git add crates/indexer/tests/reorg_handling.rs
git commit -m "test(indexer): add integration test for inline rollback of derived CFs"
```

---

### Task 6: Remove rebuild calls from `reorg_ops.rs`

**Files:**

- Modify: `crates/ckbadger-store/src/reorg_ops.rs` — delete lines 1610-1647 (rebuild block)

**Step 1: Delete the three rebuild call blocks**

Remove the entire block from `// Rebuild addr_balance from live_cells` through `"Rollback cleanup token state rebuild complete"` (lines 1610-1647, plus the comment on lines 1649-1651). This is everything between the `"Rollback cleanup write batch committed"` info log and the `// Keep sync_status tip aligned` comment.

**Step 2: Verify it compiles**

Run: `cargo check -p ckbadger-store 2>&1 | head -20`
Expected: compiles (rebuild functions still exist but are no longer called from here)

**Step 3: Run existing tests**

Run: `cargo test -p ckbadger-store -- rollback 2>&1 | tail -20`
Expected: all pass

Run: `cargo test -p ckbadger-indexer --test reorg_handling 2>&1 | tail -20`
Expected: all pass (including the new derived CFs test)

**Step 4: Commit**

```bash
git add crates/ckbadger-store/src/reorg_ops.rs
git commit -m "perf(store): remove rebuild_* calls from rollback — inline deltas handle derived CFs"
```

---

### Task 7: Remove rebuild call from `indexer.rs`

**Files:**

- Modify: `crates/indexer/src/sync/indexer.rs` — delete lines 939-951

**Step 1: Delete the conditional rebuild block**

Remove the `if !self.writer.store().has_cf(CF_ADDR_TXS)` block (lines 939-951) that calls `rebuild_addr_balances_from_live_cells_with_tx_index_store`.

**Step 2: Verify it compiles**

Run: `cargo check -p ckbadger-indexer 2>&1 | head -20`
Expected: compiles (CF_ADDR_TXS import may now be unused — remove if needed)

**Step 3: Commit**

```bash
git add crates/indexer/src/sync/indexer.rs
git commit -m "perf(indexer): remove duplicate addr_balance rebuild from startup cleanup"
```

---

### Task 8: Remove rebuild function definitions

**Files:**

- Modify: `crates/ckbadger-store/src/address_ops.rs` — delete `rebuild_addr_balances_from_live_cells` (lines 133-141) and `rebuild_addr_balances_from_live_cells_with_tx_index_store` (lines 143-393)
- Modify: `crates/ckbadger-store/src/stats_ops.rs` — delete `rebuild_script_infos_from_cells` (lines 576-1003)
- Modify: `crates/ckbadger-store/src/token_ops.rs` — delete `rebuild_token_state_from_transfers` (lines 829-1145) and `TokenStateRebuildResult` struct (lines 27-36)

**Step 1: Delete function from address_ops.rs**

Remove `rebuild_addr_balances_from_live_cells` (lines 133-141) and `rebuild_addr_balances_from_live_cells_with_tx_index_store` (lines 143-393). Keep any surrounding functions intact.

**Step 2: Delete function from stats_ops.rs**

Remove `rebuild_script_infos_from_cells` (lines 576-1003).

**Step 3: Delete function and struct from token_ops.rs**

Remove `TokenStateRebuildResult` struct (lines 27-36) and `rebuild_token_state_from_transfers` (lines 829-1145).

Also check if `TokenStateRebuildResult` is exported from `lib.rs` or `mod.rs` and remove the export.

**Step 4: Fix any remaining references**

Run: `cargo check -p ckbadger-store 2>&1 | head -40`

Fix any compilation errors from dangling imports, unused imports, or references. Common things to check:

- `use` statements in test modules that reference removed types
- `pub use` in `lib.rs` for `TokenStateRebuildResult`
- Any other call sites found by the compiler

**Step 5: Commit**

```bash
git add crates/ckbadger-store/src/address_ops.rs crates/ckbadger-store/src/stats_ops.rs crates/ckbadger-store/src/token_ops.rs
git commit -m "refactor(store): remove rebuild_addr_balances, rebuild_script_infos, rebuild_token_state functions"
```

---

### Task 9: Remove dead tests for rebuild functions

**Files:**

- Modify: `crates/ckbadger-store/src/address_ops.rs` — remove `test_addr_balance_roundtrip_split_store` (~line 545 area)
- Modify: `crates/ckbadger-store/src/stats_ops.rs` — remove tests that call `rebuild_script_infos_from_cells`: `test_rebuild_script_infos_from_cells_preserves_metadata_and_recomputes_usage` (~line 1240), `test_rebuild_script_infos_from_cells_fails_on_invalid_consumed_key_length` (~line 1311), `test_rebuild_script_infos_from_cells_fails_on_legacy_consumed_payload` (~line 1355)

**Step 1: Remove rebuild-specific tests**

Search for tests that reference the deleted functions. Remove only tests that ONLY test the removed functions. Do not remove tests that happen to use the same data structures for other purposes.

Run: `grep -n 'rebuild_addr_balances\|rebuild_script_infos\|rebuild_token_state\|TokenStateRebuildResult' crates/ckbadger-store/src/*.rs`

Delete each test function identified.

**Step 2: Verify it compiles and all tests pass**

Run: `cargo test -p ckbadger-store 2>&1 | tail -30`
Expected: all remaining tests pass

**Step 3: Commit**

```bash
git add crates/ckbadger-store/src/address_ops.rs crates/ckbadger-store/src/stats_ops.rs crates/ckbadger-store/src/token_ops.rs
git commit -m "test(store): remove dead tests for deleted rebuild_* functions"
```

---

### Task 10: Final verification

**Step 1: Run full store tests**

Run: `cargo test -p ckbadger-store 2>&1 | tail -30`
Expected: all pass

**Step 2: Run full indexer tests**

Run: `cargo test -p ckbadger-indexer 2>&1 | tail -30`
Expected: all pass

**Step 3: Run clippy**

Run: `cargo clippy -p ckbadger-store -p ckbadger-indexer 2>&1 | tail -30`
Expected: no warnings from changed files

**Step 4: Run the full pre-commit check**

Run: `cargo check && cargo clippy`
Expected: clean

**Step 5: Commit any final fixups**

If clippy or tests surface issues, fix and commit.

---

## Key Invariants to Verify During Implementation

1. **Delta maps are declared BEFORE the `if !use_tx_context` branch** so both paths populate them
2. **`accumulate_cell_deltas` is called at ALL 4 cell-state-change points** (fallback remove, fallback restore, undo-log remove, undo-log restore)
3. **Delta application happens BEFORE `self.write_batch(batch)`** so it's part of the same atomic commit
4. **No `unwrap_or(0)` or `saturating_sub` on correctness paths** — fail fast on underflow per CLAUDE.md
5. **`total_cells_count`, `txs_count`, `first_seen`, `last_activity` are NOT modified** — they derive from append-only data
6. **Stage 7 already handles hourly token stats cleanup** — stage 9d only updates total transfers count
