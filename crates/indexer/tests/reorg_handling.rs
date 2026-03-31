//! Integration tests for chain reorganization (rollback) handling via ckbadger-store.

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::{
    AddressBalance, FiberChannel, FiberChannelState, ParticipantDelta, ScriptInfo,
    ScriptReferenceInfo, TokenInfo, TxActions,
};
use ckbadger_store::CkbadgerStore;
use ckbadger_store::{
    CachedBlockHeader, DeepForkInfo, LiveCellInfo, PositionedCellInfo, RollbackResult, TxIndexEntry,
};
use std::sync::Arc;

fn setup_store() -> Arc<CkbadgerStore> {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap());
    std::mem::forget(dir);
    store
}

fn setup_split_stores() -> (Arc<CkbadgerStore>, Arc<CkbadgerStore>) {
    let domain_dir = tempfile::tempdir().unwrap();
    let append_dir = tempfile::tempdir().unwrap();
    let domain = Arc::new(CkbadgerStore::open_domain(domain_dir.path()).unwrap());
    let append = Arc::new(CkbadgerStore::open_append_only(append_dir.path()).unwrap());
    std::mem::forget(domain_dir);
    std::mem::forget(append_dir);
    (domain, append)
}

fn make_header(block_num: i64) -> CachedBlockHeader {
    let mut hash = vec![0u8; 32];
    hash[0..8].copy_from_slice(&block_num.to_le_bytes());
    CachedBlockHeader {
        hash,
        parent_hash: vec![0u8; 32],
        timestamp: 1_000_000 + block_num * 1000,
        epoch_number: block_num / 1800,
        epoch_index: (block_num % 1800) as i32,
        epoch_length: 1800,
        dao: vec![0u8; 32],
        transactions_count: 2,
        cycles: None,
    }
}

fn make_tx_entry(is_cellbase: bool) -> TxIndexEntry {
    TxIndexEntry {
        is_cellbase,
        timestamp: 1_000_000,
        inputs_count: if is_cellbase { 0 } else { 2 },
        outputs_count: 3,
        fee: if is_cellbase { 0 } else { 1000 },
        tx_size: 512,
        cycles: Some(100_000),
        semantic_tags: 0,
    }
}

fn make_cell(block_num: i64, lock_hash: &[u8]) -> PositionedCellInfo {
    PositionedCellInfo::new(
        LiveCellInfo {
            capacity: 10_000_000_000,
            lock_script_hash: lock_hash.to_vec(),
            lock_code_hash: vec![0xAA; 32],
            lock_hash_type: 1,
            lock_args: vec![0xBB; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 0,
            udt_amount: None,
            data_hash: None,
        },
        block_num,
    )
}

/// Insert a fully populated block (header + txs + cells + indexes).
/// `domain_store` holds headers, tx indexes, live cell markers, lock indexes.
/// `cells_store` holds cell payloads (CF_CELLS). If None, uses `domain_store`
/// via the combined `put_cell` method (for unified test stores).
fn insert_full_block(
    domain_store: &CkbadgerStore,
    cells_store: Option<&CkbadgerStore>,
    block_num: i64,
    lock_hash: &[u8],
) {
    let header = make_header(block_num);
    let cellbase = make_tx_entry(true);
    let normal_tx = make_tx_entry(false);

    let mut cellbase_hash = vec![0u8; 32];
    cellbase_hash[0..8].copy_from_slice(&block_num.to_le_bytes());
    cellbase_hash[8] = 0x00; // tx index 0

    let mut tx_hash = vec![0u8; 32];
    tx_hash[0..8].copy_from_slice(&block_num.to_le_bytes());
    tx_hash[8] = 0x01; // tx index 1

    let cell = make_cell(block_num, lock_hash);

    let mut domain_batch = StoreBatch::new(domain_store);
    domain_batch.put_block_header(block_num, &header);
    domain_batch.put_tx_index(block_num, 0, &cellbase);
    domain_batch.put_tx_index(block_num, 1, &normal_tx);
    domain_batch.put_tx_hash_map(&cellbase_hash, block_num, 0);
    domain_batch.put_tx_hash_map(&tx_hash, block_num, 1);

    if let Some(cs) = cells_store {
        // Split stores: payload to append-only, marker to domain
        let mut cells_batch = StoreBatch::new(cs);
        cells_batch.put_cell_payload_by_outpoint(&tx_hash, 0, &cell.cell);
        cells_batch.commit().unwrap();
        domain_batch.put_live_cell_marker_by_outpoint(&tx_hash, 0, cell.created_at_block);
    } else {
        // Unified test store: combined write
        domain_batch.put_cell(&tx_hash, 0, &cell.cell, cell.created_at_block);
    }
    domain_batch.put_cell_by_lock(lock_hash, block_num, &tx_hash, 0);
    domain_batch.commit().unwrap();
}

/// Write the derived CF records (addr_balance + script_info) that forward sync
/// would have produced. Rollback's inline delta code reads these CFs, so they
/// must exist before `rollback_to_block_with_append_only_store` is called.
fn populate_derived_cfs(store: &CkbadgerStore, lock_hash: &[u8], block_count: i64) {
    let lock_code_hash = vec![0xAA; 32]; // matches make_cell
    let cap_per_cell: i128 = 10_000_000_000; // matches make_cell

    let mut batch = StoreBatch::new(store);

    let addr_bal = AddressBalance {
        balance: block_count as i128 * cap_per_cell,
        used_capacity: 0, // make_cell sets occupied_capacity = 0
        live_cells_count: block_count as i32,
        total_cells_count: block_count,
        txs_count: block_count,
        first_seen_block: 1,
        first_seen_tx: vec![0u8; 32],
        last_activity_block: block_count,
        last_activity_tx: vec![0u8; 32],
    };
    batch.put_addr_balance(lock_hash, &addr_bal);

    let lock_si = ScriptInfo {
        code_hash: lock_code_hash.clone(),
        hash_type: 1,
        lock_live_cells_count: block_count,
        lock_owned_capacity_sum: block_count as i128 * cap_per_cell,
        ..Default::default()
    };
    batch.put_script_info(&lock_code_hash, &lock_si);

    batch.commit().unwrap();
}

#[test]
fn test_reorg_reinsert_same_outpoint_refreshes_created_at_block() {
    let (domain, append) = setup_split_stores();
    let tx_hash = vec![0xAB; 32];
    let lock_hash = vec![0xCD; 32];
    let outpoint_key = ckbadger_store::keys::encode_outpoint(&tx_hash, 0);

    let original = make_cell(100, &lock_hash);
    let mut original_cells_batch = StoreBatch::new(&append);
    original_cells_batch.put_cell_payload_by_outpoint(&tx_hash, 0, &original.cell);
    original_cells_batch.commit().unwrap();

    let mut original_domain_batch = StoreBatch::new(&domain);
    original_domain_batch.put_live_cell_marker_by_outpoint(&tx_hash, 0, original.created_at_block);
    original_domain_batch.put_cell_by_lock(&lock_hash, original.created_at_block, &tx_hash, 0);
    original_domain_batch.commit().unwrap();

    let live_before = domain.get_cell(&tx_hash, 0, &append).unwrap().unwrap();
    assert_eq!(live_before.created_at_block, 100);

    let mut rollback_batch = StoreBatch::new(&domain);
    rollback_batch.delete_cell_raw_key(&outpoint_key);
    rollback_batch.delete_cell_by_lock(&lock_hash, original.created_at_block, &tx_hash, 0);
    rollback_batch.commit().unwrap();

    let mut reorged = make_cell(101, &lock_hash);
    reorged.cell.capacity = original.capacity;
    reorged.cell.lock_script_hash = original.lock_script_hash.clone();
    reorged.cell.lock_code_hash = original.lock_code_hash.clone();
    reorged.cell.lock_hash_type = original.lock_hash_type;
    reorged.cell.lock_args = original.lock_args.clone();
    reorged.cell.type_script_hash = original.type_script_hash.clone();
    reorged.cell.type_code_hash = original.type_code_hash.clone();
    reorged.cell.type_hash_type = original.type_hash_type;
    reorged.cell.type_args = original.type_args.clone();
    reorged.cell.data_size = original.data_size;
    reorged.cell.occupied_capacity = original.occupied_capacity;
    reorged.cell.udt_amount = original.udt_amount;

    let mut reorg_cells_batch = StoreBatch::new(&append);
    reorg_cells_batch.put_cell_payload_by_outpoint(&tx_hash, 0, &reorged.cell);
    reorg_cells_batch.commit().unwrap();

    let mut reorg_domain_batch = StoreBatch::new(&domain);
    reorg_domain_batch.put_live_cell_marker_by_outpoint(&tx_hash, 0, reorged.created_at_block);
    reorg_domain_batch.put_cell_by_lock(&lock_hash, reorged.created_at_block, &tx_hash, 0);
    reorg_domain_batch.commit().unwrap();

    let live_after = domain.get_cell(&tx_hash, 0, &append).unwrap().unwrap();
    assert_eq!(live_after.created_at_block, 101);
    assert_eq!(live_after.capacity, original.capacity);
    assert_eq!(live_after.lock_script_hash, original.lock_script_hash);
}

#[test]
fn test_rollback_removes_blocks() {
    let (store, append_store) = setup_split_stores();
    let lock_hash = vec![0xFF; 32];

    // Insert blocks 1 through 10
    for i in 1..=10 {
        insert_full_block(&store, Some(append_store.as_ref()), i, &lock_hash);
    }

    // Verify all 10 blocks exist
    for i in 1..=10 {
        assert!(
            store.get_block_header(i).unwrap().is_some(),
            "block {} should exist before rollback",
            i
        );
    }

    // Populate derived CFs so inline delta code finds the records it needs.
    populate_derived_cfs(&store, &lock_hash, 10);

    // Rollback to block 5: blocks 6-10 should be removed
    let result: RollbackResult = store
        .rollback_to_block_with_append_only_store(5, Some(append_store.as_ref()))
        .unwrap();
    assert_eq!(result.blocks_removed, 5, "should remove blocks 6-10");

    // Blocks 1-5 should still exist
    for i in 1..=5 {
        assert!(
            store.get_block_header(i).unwrap().is_some(),
            "block {} should survive rollback",
            i
        );
    }

    // Blocks 6-10 should be gone
    for i in 6..=10 {
        assert!(
            store.get_block_header(i).unwrap().is_none(),
            "block {} should be removed after rollback",
            i
        );
    }
}

#[test]
fn test_rollback_removes_transactions() {
    let (store, append_store) = setup_split_stores();
    let lock_hash = vec![0xEE; 32];

    // Insert blocks 1 through 6
    for i in 1..=6 {
        insert_full_block(&store, Some(append_store.as_ref()), i, &lock_hash);
    }

    // Verify txs in block 6 exist
    let txs = store.list_block_txs(6).unwrap();
    assert_eq!(txs.len(), 2, "block 6 should have 2 txs before rollback");

    // Populate derived CFs so inline delta code finds the records it needs.
    populate_derived_cfs(&store, &lock_hash, 6);

    // Rollback to block 3
    let result = store
        .rollback_to_block_with_append_only_store(3, Some(append_store.as_ref()))
        .unwrap();
    // Blocks 4, 5, 6 removed => 3 blocks, each with 2 txs => 6 txs removed
    assert_eq!(result.blocks_removed, 3);
    assert_eq!(result.txs_removed, 6);

    // Block 6 txs should be gone
    let txs = store.list_block_txs(6).unwrap();
    assert_eq!(txs.len(), 0, "block 6 txs should be removed");

    // Block 3 txs should survive
    let txs = store.list_block_txs(3).unwrap();
    assert_eq!(txs.len(), 2, "block 3 txs should survive");
}

#[test]
fn test_rollback_removes_cells_and_indexes() {
    let (store, append_store) = setup_split_stores();
    let lock_hash = vec![0xDD; 32];

    // Insert blocks 1 through 4
    for i in 1..=4 {
        insert_full_block(&store, Some(append_store.as_ref()), i, &lock_hash);
    }

    // Verify cells exist via lock index (4 blocks, 1 cell each)
    let cells_before = store
        .list_cells_by_lock(&lock_hash, 100, None, &append_store)
        .unwrap();
    assert_eq!(cells_before.len(), 4, "should have 4 cells before rollback");

    // Populate derived CFs so inline delta code finds the records it needs.
    populate_derived_cfs(&store, &lock_hash, 4);

    // Rollback to block 2: blocks 3-4 removed
    let result = store
        .rollback_to_block_with_append_only_store(2, Some(append_store.as_ref()))
        .unwrap();
    assert_eq!(result.blocks_removed, 2);
    assert_eq!(result.cells_removed, 2, "cells from blocks 3-4 removed");

    // Cells from blocks 1-2 should survive via lock index
    let cells_after = store
        .list_cells_by_lock(&lock_hash, 100, None, &append_store)
        .unwrap();
    assert_eq!(
        cells_after.len(),
        2,
        "should have 2 cells after rollback (blocks 1-2)"
    );
}

#[test]
fn test_rollback_result_counts() {
    let (store, append_store) = setup_split_stores();
    let lock_hash = vec![0xCC; 32];

    // Insert blocks 1 through 8
    for i in 1..=8 {
        insert_full_block(&store, Some(append_store.as_ref()), i, &lock_hash);
    }

    // Populate derived CFs so inline delta code finds the records it needs.
    populate_derived_cfs(&store, &lock_hash, 8);

    // Rollback to block 5: remove blocks 6, 7, 8
    let result = store
        .rollback_to_block_with_append_only_store(5, Some(append_store.as_ref()))
        .unwrap();

    assert_eq!(result.blocks_removed, 3, "3 blocks removed (6, 7, 8)");
    assert_eq!(
        result.txs_removed, 6,
        "6 txs removed (2 per block * 3 blocks)"
    );
    assert_eq!(
        result.cells_removed, 3,
        "3 cells removed (1 per block * 3 blocks)"
    );
    // cells_restored depends on consumed cell history, which we don't
    // populate in this test, so it stays 0
    assert_eq!(result.cells_restored, 0);
}

#[test]
fn test_deep_fork_flag() {
    let store = setup_store();

    // Initially no deep fork
    assert!(!store.has_unresolved_deep_fork().unwrap());

    // Set a deep fork
    let fork_info = DeepForkInfo {
        db_tip: 1000,
        db_tip_hash: vec![0x11; 32],
        chain_tip: 995,
        chain_tip_hash: vec![0x22; 32],
        depth: 12,
        fork_point: 988,
    };
    store.set_deep_fork(fork_info).unwrap();

    assert!(store.has_unresolved_deep_fork().unwrap());

    let info = store.get_deep_fork_info().unwrap().unwrap();
    assert_eq!(info.db_tip, 1000);
    assert_eq!(info.depth, 12);
    assert_eq!(info.fork_point, 988);

    // Clear deep fork
    store.clear_deep_fork().unwrap();
    assert!(!store.has_unresolved_deep_fork().unwrap());
}

#[test]
fn test_rollback_deletes_activities_for_rolled_back_blocks() {
    let root = tempfile::tempdir().unwrap();
    let domain = CkbadgerStore::open_domain(root.path().join("domain")).unwrap();
    let append = CkbadgerStore::open_append_only(root.path().join("append-only")).unwrap();
    let lock_hash = vec![0xAA; 32];

    // Insert activities at blocks 100, 200, 300, 400, 500 into domain store
    let mut domain_batch = StoreBatch::new(&domain);
    for i in 1..=5i64 {
        let block = i * 100;
        let tx_hash = vec![i as u8; 32];
        let tx_actions = TxActions {
            tx_hash: tx_hash.clone(),
            block_hash: vec![0xD0 | (i as u8); 32],
            block_number: block,
            tx_index: 0,
            timestamp: 1_700_000_000 + block,
            is_cellbase: false,
            protocol_actions: vec![],
            type_calls: vec![],
            lock_calls: vec![],
            participants: vec![ParticipantDelta {
                lock_hash: lock_hash.clone(),
                ckb_delta: block as i128 * 100_000_000,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
            }],
        };
        domain_batch.put_tx_actions(&tx_actions);
        domain_batch.put_addr_tx(&lock_hash, block, 0, &tx_hash);
    }
    // Also insert block headers so rollback_to_block works
    for i in 1..=5i64 {
        let block = i * 100;
        domain_batch.put_block_header(block, &make_header(block));
    }
    domain_batch.commit().unwrap();

    // Verify all 5 exist
    let before = domain.list_activities(&lock_hash, 100, None, None).unwrap();
    assert_eq!(before.len(), 5);

    // Rollback to block 300: activities at blocks 400, 500 should be deleted
    domain
        .rollback_to_block_with_append_only_store(300, Some(&append))
        .unwrap();

    let after = domain.list_activities(&lock_hash, 100, None, None).unwrap();
    assert_eq!(
        after.len(),
        3,
        "activities at blocks > 300 should be deleted"
    );
    // Remaining activities in descending block order
    assert_eq!(after[0].block_number, 300);
    assert_eq!(after[1].block_number, 200);
    assert_eq!(after[2].block_number, 100);
}

/// Create a UDT cell with type_script_hash, type_code_hash, and udt_amount set.
fn make_udt_cell(block_num: i64, lock_hash: &[u8]) -> PositionedCellInfo {
    PositionedCellInfo::new(
        LiveCellInfo {
            capacity: 14_200_000_000,
            lock_script_hash: lock_hash.to_vec(),
            lock_code_hash: vec![0xAA; 32],
            lock_hash_type: 1,
            lock_args: vec![0xBB; 20],
            type_script_hash: Some(vec![0xCC; 32]),
            type_code_hash: Some(vec![0xDD; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![0xEE; 20]),
            data_size: 16,
            occupied_capacity: 14_200_000_000,
            udt_amount: Some(500_000_000),
            data_hash: None,
        },
        block_num,
    )
}

/// Insert a UDT cell into the store with a distinct tx_hash (byte [8]=0xFF).
fn insert_udt_cell(
    domain_store: &CkbadgerStore,
    cells_store: Option<&CkbadgerStore>,
    block_num: i64,
    lock_hash: &[u8],
) {
    let mut tx_hash = vec![0u8; 32];
    tx_hash[0..8].copy_from_slice(&block_num.to_le_bytes());
    tx_hash[8] = 0xFF; // distinguish from regular tx hashes

    let cell = make_udt_cell(block_num, lock_hash);
    let type_hash = cell.type_script_hash.as_ref().unwrap();
    let type_code_hash = cell.type_code_hash.as_ref().unwrap();

    let mut domain_batch = StoreBatch::new(domain_store);

    if let Some(cs) = cells_store {
        let mut cells_batch = StoreBatch::new(cs);
        cells_batch.put_cell_payload_by_outpoint(&tx_hash, 0, &cell.cell);
        cells_batch.commit().unwrap();
        domain_batch.put_live_cell_marker_by_outpoint(&tx_hash, 0, cell.created_at_block);
    } else {
        domain_batch.put_cell(&tx_hash, 0, &cell.cell, cell.created_at_block);
    }
    domain_batch.put_cell_by_lock(lock_hash, block_num, &tx_hash, 0);
    domain_batch.put_cell_by_type(type_hash, block_num, &tx_hash, 0);
    domain_batch.put_cell_by_type_code(type_code_hash, block_num, &tx_hash, 0);
    domain_batch.commit().unwrap();
}

#[test]
fn test_rollback_updates_derived_cfs_inline() {
    let (store, append_store) = setup_split_stores();
    let lock_hash = vec![1u8; 32];
    let lock_code_hash = vec![0xAA; 32];
    let type_script_hash = vec![0xCC; 32];
    let type_code_hash = vec![0xDD; 32];

    let reg_cap: i128 = 10_000_000_000;
    let udt_cap: i128 = 14_200_000_000;
    let udt_amount: i128 = 500_000_000;

    // 1. Insert blocks 1-4 with regular cells for lock_hash.
    for i in 1..=4 {
        insert_full_block(&store, Some(append_store.as_ref()), i, &lock_hash);
    }

    // 2. Insert UDT cells for blocks 3 and 4.
    insert_udt_cell(&store, Some(append_store.as_ref()), 3, &lock_hash);
    insert_udt_cell(&store, Some(append_store.as_ref()), 4, &lock_hash);

    // 3. Write initial derived CF state.
    let mut batch = StoreBatch::new(&store);

    // addr_balance: 4 regular cells + 2 UDT cells = 6 live cells
    let addr_bal = AddressBalance {
        balance: 4 * reg_cap + 2 * udt_cap,
        used_capacity: 2 * udt_cap, // only UDT cells have occupied_capacity set
        live_cells_count: 6,
        total_cells_count: 6,
        txs_count: 4,
        first_seen_block: 1,
        first_seen_tx: vec![0u8; 32],
        last_activity_block: 4,
        last_activity_tx: vec![0u8; 32],
    };
    batch.put_addr_balance(&lock_hash, &addr_bal);

    // script_info for lock_code_hash [0xAA;32]
    let lock_si = ScriptInfo {
        code_hash: lock_code_hash.clone(),
        hash_type: 1,
        lock_live_cells_count: 6,
        lock_owned_capacity_sum: 4 * reg_cap + 2 * udt_cap,
        lock_owned_knowledge_sum: 2 * udt_cap,
        ..Default::default()
    };
    batch.put_script_info(&lock_code_hash, &lock_si);

    // script_info for type_code_hash [0xDD;32]
    let type_si = ScriptInfo {
        code_hash: type_code_hash.clone(),
        hash_type: 1,
        type_live_cells_count: 2,
        type_owned_capacity_sum: 2 * udt_cap,
        type_owned_knowledge_sum: 2 * udt_cap,
        ..Default::default()
    };
    batch.put_script_info(&type_code_hash, &type_si);

    // token_holder: (type_script_hash, lock_hash) -> balance = 2 * udt_amount
    batch.put_token_holder(&type_script_hash, &lock_hash, 2 * udt_amount);

    // token_info for type_script_hash
    let token_info = TokenInfo {
        type_code_hash: type_code_hash.clone(),
        hash_type: 1,
        type_args: vec![0xEE; 20],
        standard: "xudt".to_string(),
        name: None,
        symbol: None,
        decimals: None,
        total_supply: Some(2 * udt_amount),
        max_supply: None,
        holders_count: 1,
        first_seen_block: 3,
        icon_url: None,
        description: None,
        transfers_count: 0,
    };
    batch.put_token(&type_script_hash, &token_info);

    batch.commit().unwrap();

    // 4. Rollback to block 2 — removes cells from blocks 3 and 4:
    //    2 regular cells + 2 UDT cells removed.
    let result = store
        .rollback_to_block_with_append_only_store(2, Some(append_store.as_ref()))
        .unwrap();
    assert_eq!(result.blocks_removed, 2, "blocks 3 and 4 removed");
    // 2 regular cells + 2 UDT cells = 4 cells removed
    assert_eq!(result.cells_removed, 4, "4 cells removed (2 reg + 2 UDT)");

    // 5. Assert derived CF state after rollback.

    // addr_balance: should reflect only blocks 1-2 regular cells
    let ab = store.get_addr_balance(&lock_hash).unwrap().unwrap();
    assert_eq!(
        ab.live_cells_count, 2,
        "addr_balance live_cells_count: 6 - 4 = 2"
    );
    assert_eq!(
        ab.balance,
        2 * reg_cap,
        "addr_balance balance: only 2 regular cells remain"
    );
    assert_eq!(
        ab.used_capacity, 0,
        "addr_balance used_capacity: UDT cells removed"
    );
    // txs_count is not tracked by inline rollback deltas; the rebuild path
    // recomputes it from addr_txs which is not populated in this test.

    // script_info for lock_code_hash: only 2 regular cells remain as lock
    let lock_si = store.get_script_info(&lock_code_hash).unwrap().unwrap();
    assert_eq!(
        lock_si.lock_live_cells_count, 2,
        "lock script_info: 6 - 4 = 2 live cells"
    );
    assert_eq!(
        lock_si.lock_owned_capacity_sum,
        2 * reg_cap,
        "lock script_info: capacity of 2 regular cells"
    );

    // script_info for type_code_hash: both UDT cells removed
    let type_si = store.get_script_info(&type_code_hash).unwrap().unwrap();
    assert_eq!(
        type_si.type_live_cells_count, 0,
        "type script_info: 2 - 2 = 0 live cells"
    );
    assert_eq!(
        type_si.type_owned_capacity_sum, 0,
        "type script_info: capacity 0 after all UDT cells removed"
    );

    // token_holder: balance reached 0, should be deleted
    let holder = store
        .get_token_holder_balance(&type_script_hash, &lock_hash)
        .unwrap();
    assert!(
        holder.is_none(),
        "token_holder should be deleted when balance reaches 0"
    );

    // token_info: the inline delta sets holders_count=0 and total_supply=Some(0),
    // then the rebuild_token_state_from_transfers runs and finds no transfers
    // for this type_hash, deleting the token_info entirely.
    // When the rebuild is removed (later task), this should become:
    //   ti.holders_count == 0, ti.total_supply == Some(0)
    let ti = store.get_token(&type_script_hash).unwrap();
    match ti {
        None => {
            // Rebuild path deleted the token (no token_transfers in test).
        }
        Some(ref info) => {
            // Inline-only path: verify the deltas were applied correctly.
            assert_eq!(info.holders_count, 0, "token_info holders_count: 1 - 1 = 0");
            assert_eq!(
                info.total_supply,
                Some(0),
                "token_info total_supply: 2*udt_amount - 2*udt_amount = 0"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Fiber channel rollback
// ---------------------------------------------------------------------------

/// Helper: create a FiberChannel for testing.
fn make_fiber_channel(
    funding_tx_hash: &[u8],
    output_index: u32,
    state: FiberChannelState,
    capacity: u64,
    open_block: i64,
    participants: Vec<Vec<u8>>,
    funding_lock_args: Vec<u8>,
) -> FiberChannel {
    FiberChannel {
        funding_tx_hash: funding_tx_hash.to_vec(),
        funding_output_index: output_index,
        state,
        capacity,
        udt_type_hash: None,
        udt_amount: None,
        open_block,
        open_timestamp: open_block * 1000,
        close_tx_hash: None,
        close_block: None,
        close_timestamp: None,
        commitment_tx_hash: None,
        commitment_output_index: None,
        delay_epoch: None,
        settlement_tx_hash: None,
        settlement_block: None,
        settlement_timestamp: None,
        participants,
        funding_lock_args,
    }
}

#[test]
fn test_rollback_deletes_fiber_channels_opened_after_fork_point() {
    let root = tempfile::tempdir().unwrap();
    let domain = CkbadgerStore::open_domain(root.path().join("domain")).unwrap();
    let append = CkbadgerStore::open_append_only(root.path().join("append-only")).unwrap();

    // Write block headers 1..6 so rollback knows the chain height
    let mut batch = StoreBatch::new(&domain);
    for b in 1..=6i64 {
        batch.put_block_header(b, &make_header(b));
    }

    let participant_a = vec![0xA0; 32];
    let participant_b = vec![0xB0; 32];

    // Channel A: opened at block 3 (survives rollback to 4)
    let funding_tx_a = vec![0x01; 32];
    let channel_id_a = ckbadger_store::keys::encode_fiber_channel_id(&funding_tx_a, 0);
    let channel_a = make_fiber_channel(
        &funding_tx_a,
        0,
        FiberChannelState::Open,
        300_00000000,
        3,
        vec![participant_a.clone()],
        vec![0xFA; 20],
    );

    // Channel B: opened at block 5 (should be deleted on rollback to 4)
    let funding_tx_b = vec![0x02; 32];
    let channel_id_b = ckbadger_store::keys::encode_fiber_channel_id(&funding_tx_b, 0);
    let channel_b = make_fiber_channel(
        &funding_tx_b,
        0,
        FiberChannelState::Open,
        500_00000000,
        5,
        vec![participant_b.clone()],
        vec![0xFB; 20],
    );

    batch.put_fiber_channel(&channel_id_a, &channel_a);
    batch.put_addr_fiber_channel(&participant_a, &channel_id_a);
    batch.put_fiber_channel_by_funding_args(&channel_a.funding_lock_args, &channel_id_a);

    batch.put_fiber_channel(&channel_id_b, &channel_b);
    batch.put_addr_fiber_channel(&participant_b, &channel_id_b);
    batch.put_fiber_channel_by_funding_args(&channel_b.funding_lock_args, &channel_id_b);
    batch.commit().unwrap();

    // Sanity: both channels exist
    assert!(domain.get_fiber_channel(&channel_id_a).unwrap().is_some());
    assert!(domain.get_fiber_channel(&channel_id_b).unwrap().is_some());

    // Rollback to block 4
    domain
        .rollback_to_block_with_append_only_store(4, Some(&append))
        .unwrap();

    // Channel A (opened at 3) survives
    let ch_a = domain.get_fiber_channel(&channel_id_a).unwrap();
    assert!(
        ch_a.is_some(),
        "channel opened before fork point should survive"
    );
    assert_eq!(ch_a.unwrap().state, FiberChannelState::Open);

    // Channel B (opened at 5) is deleted
    let ch_b = domain.get_fiber_channel(&channel_id_b).unwrap();
    assert!(
        ch_b.is_none(),
        "channel opened after fork point should be deleted"
    );

    // addr_fiber_channel index for participant_b should be gone
    let addr_channels_b = domain.list_addr_fiber_channels(&participant_b, 10).unwrap();
    assert!(
        addr_channels_b.is_empty(),
        "addr_fiber_channel index for deleted channel should be cleaned up"
    );

    // addr_fiber_channel for participant_a still exists
    let addr_channels_a = domain.list_addr_fiber_channels(&participant_a, 10).unwrap();
    assert_eq!(
        addr_channels_a.len(),
        1,
        "surviving channel's addr index should remain"
    );
}

#[test]
fn test_rollback_resets_fiber_channel_closed_after_fork_point_to_open() {
    let root = tempfile::tempdir().unwrap();
    let domain = CkbadgerStore::open_domain(root.path().join("domain")).unwrap();
    let append = CkbadgerStore::open_append_only(root.path().join("append-only")).unwrap();

    let participant = vec![0xA0; 32];

    // Write block headers so rollback knows chain height
    let mut batch = StoreBatch::new(&domain);
    for b in 1..=6i64 {
        batch.put_block_header(b, &make_header(b));
    }

    // Channel opened at block 2, cooperatively closed at block 5
    let funding_tx = vec![0x03; 32];
    let channel_id = ckbadger_store::keys::encode_fiber_channel_id(&funding_tx, 0);
    let mut channel = make_fiber_channel(
        &funding_tx,
        0,
        FiberChannelState::CooperativelyClosed,
        400_00000000,
        2,
        vec![participant.clone()],
        vec![0xFC; 20],
    );
    channel.close_tx_hash = Some(vec![0xCC; 32]);
    channel.close_block = Some(5);
    channel.close_timestamp = Some(5000);
    channel.commitment_tx_hash = Some(vec![0xDD; 32]);
    channel.commitment_output_index = Some(0);

    batch.put_fiber_channel(&channel_id, &channel);
    batch.put_addr_fiber_channel(&participant, &channel_id);
    // Write commitment index
    batch
        .put_fiber_channel_by_commitment(channel.commitment_tx_hash.as_ref().unwrap(), &channel_id);
    batch.commit().unwrap();

    // Rollback to block 4 — channel was opened at 2, closed at 5
    domain
        .rollback_to_block_with_append_only_store(4, Some(&append))
        .unwrap();

    // Channel should be reset to Open with close/commitment fields cleared
    let ch = domain
        .get_fiber_channel(&channel_id)
        .unwrap()
        .expect("channel should survive");
    assert_eq!(
        ch.state,
        FiberChannelState::Open,
        "state should be reset to Open"
    );
    assert!(
        ch.close_tx_hash.is_none(),
        "close_tx_hash should be cleared"
    );
    assert!(ch.close_block.is_none(), "close_block should be cleared");
    assert!(
        ch.close_timestamp.is_none(),
        "close_timestamp should be cleared"
    );
    assert!(
        ch.commitment_tx_hash.is_none(),
        "commitment_tx_hash should be cleared"
    );
    assert!(
        ch.commitment_output_index.is_none(),
        "commitment_output_index should be cleared"
    );
    assert!(
        ch.settlement_tx_hash.is_none(),
        "settlement_tx_hash should be cleared"
    );

    // Original fields preserved
    assert_eq!(ch.open_block, 2);
    assert_eq!(ch.capacity, 400_00000000);

    // Commitment index should be cleaned up
    let commitment_lookup = domain
        .get_fiber_channel_id_by_commitment(&[0xDD; 32])
        .unwrap();
    assert!(
        commitment_lookup.is_none(),
        "commitment index should be deleted for reset channel"
    );
}

// ---------------------------------------------------------------------------
// Script reference info rollback
// ---------------------------------------------------------------------------

#[test]
fn test_rollback_adjusts_script_reference_info_deltas() {
    let (domain, append) = setup_split_stores();

    let lock_hash = vec![0x11; 32];
    let lock_code_hash = vec![0xAA; 32];

    // Insert blocks 1..5 with cells using lock_code_hash
    for b in 1..=5 {
        insert_full_block(&domain, Some(&append), b, &lock_hash);
    }
    populate_derived_cfs(&domain, &lock_hash, 5);

    // Pre-populate script_reference_info for the lock code_hash.
    // hash_type=1, is_type=false (lock side).
    // Simulate 5 cells using this script as lock: 5 total, 5 live.
    let cap_per_cell: i128 = 10_000_000_000;
    let sri = ScriptReferenceInfo {
        reference_hash: lock_code_hash.clone(),
        hash_type: 1,
        lock_cells_count: 5,
        lock_live_cells_count: 5,
        lock_capacity_sum: 5 * cap_per_cell,
        lock_owned_capacity_sum: 5 * cap_per_cell,
        lock_used_capacity_sum: 0,
        lock_owned_knowledge_sum: 0,
        type_cells_count: 0,
        type_live_cells_count: 0,
        type_capacity_sum: 0,
        type_owned_capacity_sum: 0,
        type_used_capacity_sum: 0,
        type_owned_knowledge_sum: 0,
    };
    let mut batch = StoreBatch::new(&domain);
    batch.put_script_reference_info(1, &lock_code_hash, &sri);
    batch.commit().unwrap();

    // Rollback to block 2 — removes blocks 3,4,5 (3 cells)
    let result = domain
        .rollback_to_block_with_append_only_store(2, Some(&append))
        .unwrap();
    assert_eq!(result.cells_removed, 3, "3 cells in blocks 3-5");

    // Script reference info should have deltas applied:
    // 3 cells removed from lock side
    let updated = domain
        .get_script_reference_info(1, &lock_code_hash)
        .unwrap()
        .expect("reference info should still exist");

    assert_eq!(
        updated.lock_cells_count,
        5 - 3,
        "lock_cells_count: 5 - 3 removed = 2"
    );
    assert_eq!(
        updated.lock_live_cells_count,
        5 - 3,
        "lock_live_cells_count: 5 - 3 removed = 2"
    );
    assert_eq!(
        updated.lock_owned_capacity_sum,
        2 * cap_per_cell,
        "lock_owned_capacity_sum: 2 surviving cells"
    );
    // Type side untouched
    assert_eq!(updated.type_cells_count, 0);
    assert_eq!(updated.type_live_cells_count, 0);
}

// ---------------------------------------------------------------------------
// Multi-participant activity rollback
// ---------------------------------------------------------------------------

#[test]
fn test_rollback_deletes_multi_participant_activities() {
    let root = tempfile::tempdir().unwrap();
    let domain = CkbadgerStore::open_domain(root.path().join("domain")).unwrap();
    let append = CkbadgerStore::open_append_only(root.path().join("append-only")).unwrap();

    let lock_a = vec![0xA0; 32];
    let lock_b = vec![0xB0; 32];

    // Write block headers and activities directly (no cells needed for this test)
    let mut batch = StoreBatch::new(&domain);
    for b in 1..=5i64 {
        batch.put_block_header(b, &make_header(b));
    }
    batch.commit().unwrap();

    // Create activities: block 2 has single-participant, blocks 3 and 4 have
    // multi-participant activities (both lock_a and lock_b).
    let mut batch = StoreBatch::new(&domain);

    // Block 2, tx 0: single participant (lock_a only) — should survive rollback to 2
    let mut tx_hash_2 = vec![0u8; 32];
    tx_hash_2[0] = 0x20;
    let actions_2 = TxActions {
        tx_hash: tx_hash_2.clone(),
        block_hash: make_header(2).hash,
        block_number: 2,
        tx_index: 0,
        timestamp: 2000,
        is_cellbase: false,
        protocol_actions: vec![],
        type_calls: vec![],
        lock_calls: vec![],
        participants: vec![ParticipantDelta {
            lock_hash: lock_a.clone(),
            ckb_delta: -5_000_000_000,
            used_delta: 0,
            item_deltas: vec![],
            tags: 0,
        }],
    };
    batch.put_tx_actions(&actions_2);
    batch.put_addr_tx(&lock_a, 2, 0, &tx_hash_2);

    // Block 3, tx 0: two participants — should be deleted on rollback to 2
    let mut tx_hash_3 = vec![0u8; 32];
    tx_hash_3[0] = 0x30;
    let actions_3 = TxActions {
        tx_hash: tx_hash_3.clone(),
        block_hash: make_header(3).hash,
        block_number: 3,
        tx_index: 0,
        timestamp: 3000,
        is_cellbase: false,
        protocol_actions: vec![],
        type_calls: vec![],
        lock_calls: vec![],
        participants: vec![
            ParticipantDelta {
                lock_hash: lock_a.clone(),
                ckb_delta: -10_000_000_000,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
            },
            ParticipantDelta {
                lock_hash: lock_b.clone(),
                ckb_delta: 10_000_000_000,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
            },
        ],
    };
    batch.put_tx_actions(&actions_3);
    batch.put_addr_tx(&lock_a, 3, 0, &tx_hash_3);
    batch.put_addr_tx(&lock_b, 3, 0, &tx_hash_3);

    // Block 4, tx 1: another multi-participant — should also be deleted
    let mut tx_hash_4 = vec![0u8; 32];
    tx_hash_4[0] = 0x40;
    let actions_4 = TxActions {
        tx_hash: tx_hash_4.clone(),
        block_hash: make_header(4).hash,
        block_number: 4,
        tx_index: 1,
        timestamp: 4000,
        is_cellbase: false,
        protocol_actions: vec![],
        type_calls: vec![],
        lock_calls: vec![],
        participants: vec![
            ParticipantDelta {
                lock_hash: lock_a.clone(),
                ckb_delta: 5_000_000_000,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
            },
            ParticipantDelta {
                lock_hash: lock_b.clone(),
                ckb_delta: -5_000_000_000,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
            },
        ],
    };
    batch.put_tx_actions(&actions_4);
    batch.put_addr_tx(&lock_a, 4, 1, &tx_hash_4);
    batch.put_addr_tx(&lock_b, 4, 1, &tx_hash_4);

    batch.commit().unwrap();

    // Sanity: global activity list has 3 entries
    let all = domain.list_tx_actions_recent(10, None).unwrap();
    assert_eq!(all.len(), 3, "3 TxActions written");

    // Sanity: lock_a appears in all 3, lock_b in 2
    let acts_a = domain.list_activities(&lock_a, 10, None, None).unwrap();
    assert_eq!(acts_a.len(), 3, "lock_a participates in all 3");
    let acts_b = domain.list_activities(&lock_b, 10, None, None).unwrap();
    assert_eq!(acts_b.len(), 2, "lock_b participates in blocks 3 and 4");

    // Rollback to block 2 — removes blocks 3,4,5
    domain
        .rollback_to_block_with_append_only_store(2, Some(&append))
        .unwrap();

    // Global list: only block 2 activity survives
    let remaining = domain.list_tx_actions_recent(10, None).unwrap();
    assert_eq!(remaining.len(), 1, "only block 2 activity survives");
    assert_eq!(remaining[0].block_number, 2);
    assert_eq!(remaining[0].participants.len(), 1);

    // Per-address: lock_a has 1 activity (block 2)
    let acts_a_after = domain.list_activities(&lock_a, 10, None, None).unwrap();
    assert_eq!(
        acts_a_after.len(),
        1,
        "lock_a: only block 2 activity survives"
    );
    assert_eq!(acts_a_after[0].block_number, 2);

    // Per-address: lock_b has 0 activities (both were in blocks 3 and 4)
    let acts_b_after = domain.list_activities(&lock_b, 10, None, None).unwrap();
    assert_eq!(
        acts_b_after.len(),
        0,
        "lock_b: all activities were in rolled-back blocks"
    );
}
