//! Integration tests for chain reorganization (rollback) handling via ckbadger-store.

use ckbadger_common::TokenBalance;
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::{
    AddrTxValue, AddressBalance, FiberChannel, FiberChannelState, ParticipantDelta, ParticipantId,
    ScriptInfo, ScriptReferenceInfo, TokenInfo, TxActions,
};
use ckbadger_store::{commit_journaled_block, CkbadgerStore};
use ckbadger_store::{
    CachedBlockHeader, DeepForkInfo, LiveCellInfo, PositionedCellInfo, RollbackResult, TxIndexEntry,
};
use std::sync::Arc;

/// Headers `blocks`, each in its block's journaled scope, as the live writer
/// commits them (every live block journals at least its `block_hash_index`).
fn journaled_headers(store: &CkbadgerStore, blocks: std::ops::RangeInclusive<i64>) {
    ckbadger_store::commit_journaled(store, |batch| {
        for block in blocks {
            batch.at(block).put_block_header(block, &make_header(block));
        }
    })
    .unwrap();
}

/// The production rollback (the block journal) to `fork_point`.
fn rollback_fixture(
    domain: &CkbadgerStore,
    fork_point: i64,
    append: Option<&CkbadgerStore>,
) -> anyhow::Result<RollbackResult> {
    // The append-only store holds cell payloads, which a rollback never touches.
    let _ = append;
    domain.rollback_to_block(fork_point)
}

fn setup_store() -> Arc<CkbadgerStore> {
    Arc::new(CkbadgerStore::open_domain_temp().unwrap())
}

fn setup_split_stores() -> (Arc<CkbadgerStore>, Arc<CkbadgerStore>) {
    let domain = Arc::new(CkbadgerStore::open_domain_temp().unwrap());
    let append = Arc::new(CkbadgerStore::open_append_only_temp().unwrap());
    (domain, append)
}

/// `tx_hash` indexed at its position `(block, tx_idx)`: what an `addr_txs` /
/// `activities` row at that position resolves to.
fn index_tx(batch: &mut StoreBatch, block: i64, tx_idx: i32, tx_hash: &[u8]) {
    batch.put_tx_index(
        block,
        tx_idx,
        &ckbadger_store::types::TxIndexEntry {
            tx_hash: tx_hash.try_into().expect("32-byte tx hash"),
            is_cellbase: false,
            timestamp: 0,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 0,
            cycles: None,
            semantic_tags: 0,
        },
    );
}

fn make_header(block_num: i64) -> CachedBlockHeader {
    let mut hash = vec![0u8; 32];
    hash[0..8].copy_from_slice(&block_num.to_le_bytes());
    CachedBlockHeader {
        hash,
        timestamp: 1_000_000 + block_num * 1000,
        epoch_number: block_num / 1800,
        epoch_index: (block_num % 1800) as i32,
        epoch_length: 1800,
        dao: vec![0u8; 32],
        transactions_count: 2,
        uncles_count: 0,
        proposals_count: 0,
        compact_target: 0x1a08a97e,
        miner_lock_hash: None,
        cycles: None,
        chain_time_ms: 1_000_000 + block_num * 1000,
    }
}

/// Seed epoch stats rows consistent with `make_header` semantics for blocks
/// `from..=to`. Real write paths persist epoch rows atomically with their
/// blocks; rollback fails fast if a mid-epoch row is missing, so fixtures
/// must uphold the same invariant.
fn seed_epoch_rows<I: IntoIterator<Item = i64>>(store: &CkbadgerStore, blocks: I) {
    use std::collections::BTreeMap;
    let mut by_epoch: BTreeMap<i64, (i64, i64)> = BTreeMap::new();
    for b in blocks {
        let e = by_epoch.entry(b / 1800).or_insert((b, b));
        e.0 = e.0.min(b);
        e.1 = e.1.max(b);
    }
    for (epoch, (start, end)) in by_epoch {
        let existing = store.get_epoch_stats(epoch).unwrap();
        let (start_block, start_ts) = match &existing {
            Some(row) => (row.start_block.min(start), row.start_timestamp),
            None => (
                start,
                chrono::DateTime::from_timestamp_millis(1_000_000 + start * 1000).unwrap(),
            ),
        };
        let end_block = existing
            .as_ref()
            .and_then(|row| row.end_block)
            .unwrap_or(end)
            .max(end);
        let blocks_count = (end_block - start_block + 1) as i32;
        store
            .put_epoch_stats(
                epoch,
                &ckbadger_store::types::EpochStats {
                    epoch_number: epoch,
                    start_block,
                    end_block: Some(end_block),
                    blocks_count,
                    length: 1800,
                    start_timestamp: start_ts,
                    end_timestamp: None,
                    transactions_count: blocks_count * 2,
                },
            )
            .unwrap();
    }
}

fn make_tx_entry(is_cellbase: bool) -> TxIndexEntry {
    TxIndexEntry {
        tx_hash: [0u8; 32],
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

/// The address of the owner whose cells `make_cell` / `make_udt_cell` build
/// from `lock_args`: the derived hash of their lock (`[0xAA; 32]`, `type`).
fn owner_lock_hash(lock_args: &[u8]) -> Vec<u8> {
    ckbadger_indexer::parser::ScriptParser::compute_script_hash_raw(&[0xAA; 32], 1, lock_args)
}

/// `lock_args` names the owner; its address is [`owner_lock_hash`].
fn make_cell(block_num: i64, lock_args: &[u8]) -> PositionedCellInfo {
    PositionedCellInfo::new(
        LiveCellInfo {
            capacity: 10_000_000_000,
            lock_code_hash: [0xAA; 32],
            lock_hash_type: 1,
            lock_args: lock_args.to_vec(),
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
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
    lock_args: &[u8],
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

    let cell = make_cell(block_num, lock_args);

    if let Some(cs) = cells_store {
        // Split stores: payload to append-only, marker to domain
        let mut cells_batch = StoreBatch::new(cs);
        cells_batch
            .put_cell_payload_by_outpoint(&tx_hash, 0, &cell.cell)
            .unwrap();
        cells_batch.commit().unwrap();
    }
    // The block's domain rows as the live writer commits them: journaled in
    // the block's scope.
    commit_journaled_block(domain_store, block_num, |domain_batch| {
        domain_batch.put_block_header(block_num, &header);
        domain_batch.put_tx_index(block_num, 0, &cellbase);
        domain_batch.put_tx_index(block_num, 1, &normal_tx);
        domain_batch.put_tx_hash_map(&cellbase_hash, block_num, 0);
        domain_batch.put_tx_hash_map(&tx_hash, block_num, 1);
        if cells_store.is_some() {
            domain_batch.put_live_cell_marker_by_outpoint(&tx_hash, 0, cell.created_at_block);
        } else {
            // Unified test store: combined write
            domain_batch.put_cell(&tx_hash, 0, &cell.cell, cell.created_at_block);
        }
        domain_batch.put_cell_by_lock(&cell.lock_script_hash(), block_num, &tx_hash, 0);
    })
    .unwrap();
    seed_epoch_rows(domain_store, [block_num]);
}

/// Write the derived CF records (address and both script rollups) that forward sync
/// would have produced for blocks `1..=block_count` (one `make_cell` per block):
/// each block's values in that block's journaled scope, as the live writer
/// leaves them, so a rollback restores the values of its target block.
fn populate_derived_cfs(store: &CkbadgerStore, lock_args: &[u8], block_count: i64) {
    let lock_hash = owner_lock_hash(lock_args);
    let lock_hash = lock_hash.as_slice();
    let lock_code_hash = vec![0xAA; 32]; // matches make_cell
    let cap_per_cell: i128 = 10_000_000_000; // matches make_cell
                                             // make_cell's occupied capacity, derived from its lock args.
    let occupied_per_cell = i128::from(make_cell(0, lock_args).occupied_capacity());

    for block in 1..=block_count {
        let cells = block;
        commit_journaled_block(store, block, |batch| {
            let addr_bal = AddressBalance {
                balance: cells as i128 * cap_per_cell,
                used_capacity: cells as i128 * occupied_per_cell,
                live_cells_count: cells as i32,
                total_cells_count: cells,
                txs_count: cells,
                cellbase_txs_count: 0,
                first_seen_block: 1,
                first_seen_tx_idx: 0,
                last_activity_block: block,
                last_activity_tx_idx: 0,
            };
            batch.put_addr_balance(lock_hash, &addr_bal);

            let lock_si = ScriptInfo {
                code_hash: lock_code_hash.clone(),
                hash_type: 1,
                lock_cells_count: cells,
                lock_live_cells_count: cells,
                lock_capacity_sum: cells as i128 * cap_per_cell,
                lock_owned_capacity_sum: cells as i128 * cap_per_cell,
                lock_used_capacity_sum: cells as i128 * occupied_per_cell,
                lock_owned_knowledge_sum: cells as i128 * occupied_per_cell,
                ..Default::default()
            };
            batch.put_script_info(&lock_code_hash, &lock_si);
            batch.put_script_reference_info(
                1,
                &lock_code_hash,
                &ScriptReferenceInfo {
                    reference_hash: lock_code_hash.clone(),
                    hash_type: 1,
                    lock_cells_count: cells,
                    lock_live_cells_count: cells,
                    lock_capacity_sum: cells as i128 * cap_per_cell,
                    lock_owned_capacity_sum: cells as i128 * cap_per_cell,
                    lock_used_capacity_sum: cells as i128 * occupied_per_cell,
                    lock_owned_knowledge_sum: cells as i128 * occupied_per_cell,
                    ..Default::default()
                },
            );
        })
        .unwrap();
    }
}

#[test]
fn test_reorg_reinsert_same_outpoint_refreshes_created_at_block() {
    let (domain, append) = setup_split_stores();
    let tx_hash = vec![0xAB; 32];
    let lock_args = vec![0xCD; 32];
    let lock_hash = owner_lock_hash(&lock_args);
    let outpoint_key = ckbadger_store::keys::encode_outpoint(&tx_hash, 0);

    let original = make_cell(100, &lock_args);
    let mut original_cells_batch = StoreBatch::new(&append);
    original_cells_batch
        .put_cell_payload_by_outpoint(&tx_hash, 0, &original.cell)
        .unwrap();
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

    let mut reorged = make_cell(101, &lock_args);
    reorged.cell.capacity = original.capacity;
    reorged.cell.lock_code_hash = original.lock_code_hash;
    reorged.cell.lock_hash_type = original.lock_hash_type;
    reorged.cell.lock_args = original.lock_args.clone();
    reorged.cell.type_code_hash = original.type_code_hash;
    reorged.cell.type_hash_type = original.type_hash_type;
    reorged.cell.type_args = original.type_args.clone();
    reorged.cell.data_size = original.data_size;
    reorged.cell.udt_amount = original.udt_amount;

    let mut reorg_cells_batch = StoreBatch::new(&append);
    reorg_cells_batch
        .put_cell_payload_by_outpoint(&tx_hash, 0, &reorged.cell)
        .unwrap();
    reorg_cells_batch.commit().unwrap();

    let mut reorg_domain_batch = StoreBatch::new(&domain);
    reorg_domain_batch.put_live_cell_marker_by_outpoint(&tx_hash, 0, reorged.created_at_block);
    reorg_domain_batch.put_cell_by_lock(&lock_hash, reorged.created_at_block, &tx_hash, 0);
    reorg_domain_batch.commit().unwrap();

    let live_after = domain.get_cell(&tx_hash, 0, &append).unwrap().unwrap();
    assert_eq!(live_after.created_at_block, 101);
    assert_eq!(live_after.capacity, original.capacity);
    assert_eq!(live_after.lock_script_hash(), original.lock_script_hash());
}

#[test]
fn test_rollback_removes_blocks() {
    let (store, append_store) = setup_split_stores();
    let lock_args = vec![0xFF; 32];

    // Insert blocks 1 through 10
    for i in 1..=10 {
        insert_full_block(&store, Some(append_store.as_ref()), i, &lock_args);
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
    populate_derived_cfs(&store, &lock_args, 10);

    // Rollback to block 5: blocks 6-10 should be removed
    let result: RollbackResult = rollback_fixture(&store, 5, Some(append_store.as_ref())).unwrap();
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
    let lock_args = vec![0xEE; 32];

    // Insert blocks 1 through 6
    for i in 1..=6 {
        insert_full_block(&store, Some(append_store.as_ref()), i, &lock_args);
    }

    // Verify txs in block 6 exist
    let txs = store.list_block_txs(6).unwrap();
    assert_eq!(txs.len(), 2, "block 6 should have 2 txs before rollback");

    // Populate derived CFs so inline delta code finds the records it needs.
    populate_derived_cfs(&store, &lock_args, 6);

    // Rollback to block 3
    let result = rollback_fixture(&store, 3, Some(append_store.as_ref())).unwrap();
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
    let lock_args = vec![0xDD; 32];
    let lock_hash = owner_lock_hash(&lock_args);

    // Insert blocks 1 through 4
    for i in 1..=4 {
        insert_full_block(&store, Some(append_store.as_ref()), i, &lock_args);
    }

    // Verify cells exist via lock index (4 blocks, 1 cell each)
    let cells_before = store
        .list_cells_by_lock(&lock_hash, 100, None, &append_store)
        .unwrap();
    assert_eq!(cells_before.len(), 4, "should have 4 cells before rollback");

    // Populate derived CFs so inline delta code finds the records it needs.
    populate_derived_cfs(&store, &lock_args, 4);

    // Rollback to block 2: blocks 3-4 removed
    let result = rollback_fixture(&store, 2, Some(append_store.as_ref())).unwrap();
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
    let lock_args = vec![0xCC; 32];

    // Insert blocks 1 through 8
    for i in 1..=8 {
        insert_full_block(&store, Some(append_store.as_ref()), i, &lock_args);
    }

    // Populate derived CFs so inline delta code finds the records it needs.
    populate_derived_cfs(&store, &lock_args, 8);

    // Rollback to block 5: remove blocks 6, 7, 8
    let result = rollback_fixture(&store, 5, Some(append_store.as_ref())).unwrap();

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

    // Seed a contiguous header chain 1..=500. Rollback derives each orphaned
    // block's inter-block gap from its immediate predecessor's header, so a
    // sparse header range is a chain the store can never hold (the indexer
    // fails startup on any internal header gap) and rollback now says so.
    journaled_headers(&domain, 1..=500);
    seed_epoch_rows(&domain, 1..=500i64);

    // Activities at blocks 100, 200, 300, 400, 500, each written in its
    // block's journaled scope as the live writer writes them.
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
                id: ParticipantId::lock(&lock_hash).unwrap(),
                ckb_delta: block as i128 * 100_000_000,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
                roles: 0,
            }],
        };
        commit_journaled_block(&domain, block, |batch| {
            batch.put_tx_actions(&tx_actions);
            batch.put_addr_tx(&lock_hash, block, 0, &AddrTxValue::new(0, false, true, 0));
            index_tx(batch, block, 0, &tx_hash);
        })
        .unwrap();
    }

    // Verify all 5 exist
    let before = domain.list_activities(&lock_hash, 100, None, None).unwrap();
    assert_eq!(before.len(), 5);

    // Rollback to block 300: activities at blocks 400, 500 should be deleted
    rollback_fixture(&domain, 300, Some(&append)).unwrap();

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
fn make_udt_cell(block_num: i64, lock_args: &[u8]) -> PositionedCellInfo {
    PositionedCellInfo::new(
        LiveCellInfo {
            capacity: 14_200_000_000,
            lock_code_hash: [0xAA; 32],
            lock_hash_type: 1,
            lock_args: lock_args.to_vec(),
            type_code_hash: Some([0xDD; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![0xEE; 20]),
            data_size: 16,
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
    lock_args: &[u8],
) {
    let mut tx_hash = vec![0u8; 32];
    tx_hash[0..8].copy_from_slice(&block_num.to_le_bytes());
    tx_hash[8] = 0xFF; // distinguish from regular tx hashes

    let cell = make_udt_cell(block_num, lock_args);
    let type_hash = &cell.type_script_hash().unwrap();
    let type_code_hash = cell.type_code_hash.as_ref().unwrap();

    if let Some(cs) = cells_store {
        let mut cells_batch = StoreBatch::new(cs);
        cells_batch
            .put_cell_payload_by_outpoint(&tx_hash, 0, &cell.cell)
            .unwrap();
        cells_batch.commit().unwrap();
    }
    commit_journaled_block(domain_store, block_num, |domain_batch| {
        if cells_store.is_some() {
            domain_batch.put_live_cell_marker_by_outpoint(&tx_hash, 0, cell.created_at_block);
        } else {
            domain_batch.put_cell(&tx_hash, 0, &cell.cell, cell.created_at_block);
        }
        domain_batch.put_cell_by_lock(&cell.lock_script_hash(), block_num, &tx_hash, 0);
        domain_batch.put_cell_by_type(type_hash, block_num, &tx_hash, 0);
        domain_batch.put_cell_by_type_code(
            type_code_hash,
            cell.type_hash_type.unwrap() as u8,
            block_num,
            &tx_hash,
            0,
        );
    })
    .unwrap();
}

#[test]
fn test_rollback_updates_derived_cfs_inline() {
    let (store, append_store) = setup_split_stores();
    let lock_args = vec![1u8; 32];
    let lock_hash = owner_lock_hash(&lock_args);
    let lock_code_hash = vec![0xAA; 32];
    // The token of `make_udt_cell`'s cells: the derived hash of their type script.
    let type_script_hash = make_udt_cell(0, &lock_args)
        .type_script_hash()
        .unwrap()
        .to_vec();
    let type_code_hash = vec![0xDD; 32];

    let reg_cap: i128 = 10_000_000_000;
    // The UDT cell's derived occupied capacity is its whole capacity:
    // 8 + 33 + 32 lock args + 33 + 20 type args + 16 data bytes = 142 CKB.
    let udt_cap: i128 = 14_200_000_000;
    // A regular cell's derived occupied capacity: 8 + 33 + 32 lock args.
    let reg_occ = i128::from(make_cell(0, &lock_args).occupied_capacity());
    let udt_amount: u128 = 500_000_000;

    // 1. Insert blocks 1-4 with regular cells for lock_hash.
    for i in 1..=4 {
        insert_full_block(&store, Some(append_store.as_ref()), i, &lock_args);
    }

    // 2. Insert UDT cells for blocks 3 and 4.
    insert_udt_cell(&store, Some(append_store.as_ref()), 3, &lock_args);
    insert_udt_cell(&store, Some(append_store.as_ref()), 4, &lock_args);

    // 3. The derived CF state at the end of every block, written in that
    //    block's journaled scope as the live writer leaves it: block b holds
    //    b regular cells and, from block 3 on, b - 2 UDT cells.
    for block in 1..=4i64 {
        let reg = i128::from(block);
        let udt = i128::from(block - 2).max(0);
        let cells = reg + udt;
        commit_journaled_block(&store, block, |batch| {
            batch.put_addr_balance(
                &lock_hash,
                &AddressBalance {
                    balance: reg * reg_cap + udt * udt_cap,
                    used_capacity: reg * reg_occ + udt * udt_cap,
                    live_cells_count: cells as i32,
                    total_cells_count: cells as i64,
                    txs_count: block,
                    cellbase_txs_count: 0,
                    first_seen_block: 1,
                    first_seen_tx_idx: 0,
                    last_activity_block: block,
                    last_activity_tx_idx: 0,
                },
            );
            // script_info for lock_code_hash [0xAA;32]
            batch.put_script_info(
                &lock_code_hash,
                &ScriptInfo {
                    code_hash: lock_code_hash.clone(),
                    hash_type: 1,
                    lock_cells_count: cells as i64,
                    lock_live_cells_count: cells as i64,
                    lock_capacity_sum: reg * reg_cap + udt * udt_cap,
                    lock_owned_capacity_sum: reg * reg_cap + udt * udt_cap,
                    lock_used_capacity_sum: reg * reg_occ + udt * udt_cap,
                    lock_owned_knowledge_sum: reg * reg_occ + udt * udt_cap,
                    ..Default::default()
                },
            );
            batch.put_script_reference_info(
                1,
                &lock_code_hash,
                &ScriptReferenceInfo {
                    reference_hash: lock_code_hash.clone(),
                    hash_type: 1,
                    lock_cells_count: cells as i64,
                    lock_live_cells_count: cells as i64,
                    lock_capacity_sum: reg * reg_cap + udt * udt_cap,
                    lock_owned_capacity_sum: reg * reg_cap + udt * udt_cap,
                    lock_used_capacity_sum: reg * reg_occ + udt * udt_cap,
                    lock_owned_knowledge_sum: reg * reg_occ + udt * udt_cap,
                    ..Default::default()
                },
            );
            if udt == 0 {
                return;
            }
            // script_info for type_code_hash [0xDD;32], first seen at block 3
            batch.put_script_info(
                &type_code_hash,
                &ScriptInfo {
                    code_hash: type_code_hash.clone(),
                    hash_type: 1,
                    type_cells_count: udt as i64,
                    type_live_cells_count: udt as i64,
                    type_capacity_sum: udt * udt_cap,
                    type_owned_capacity_sum: udt * udt_cap,
                    type_used_capacity_sum: udt * udt_cap,
                    type_owned_knowledge_sum: udt * udt_cap,
                    ..Default::default()
                },
            );
            batch.put_script_reference_info(
                1,
                &type_code_hash,
                &ScriptReferenceInfo {
                    reference_hash: type_code_hash.clone(),
                    hash_type: 1,
                    type_cells_count: udt as i64,
                    type_live_cells_count: udt as i64,
                    type_capacity_sum: udt * udt_cap,
                    type_owned_capacity_sum: udt * udt_cap,
                    type_used_capacity_sum: udt * udt_cap,
                    type_owned_knowledge_sum: udt * udt_cap,
                    ..Default::default()
                },
            );
            // token_holder: (type_script_hash, lock_hash) -> balance
            batch.put_token_holder(&type_script_hash, &lock_hash, udt as u128 * udt_amount);
            // token_info for type_script_hash
            batch.put_token(
                &type_script_hash,
                &TokenInfo {
                    type_code_hash: type_code_hash.clone(),
                    hash_type: 1,
                    type_args: vec![0xEE; 20],
                    standard: "xudt".to_string(),
                    name: None,
                    symbol: None,
                    decimals: None,
                    max_supply: None,
                    first_seen_block: 3,
                    icon_url: None,
                    description: None,
                    transfers_count: 0,
                    metadata_source: None,
                    chain_metadata: None,
                },
            );
        })
        .unwrap();
    }

    // 4. Rollback to block 2 — removes cells from blocks 3 and 4:
    //    2 regular cells + 2 UDT cells removed.
    let result = rollback_fixture(&store, 2, Some(append_store.as_ref())).unwrap();
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
        ab.used_capacity,
        2 * reg_occ,
        "addr_balance used_capacity: UDT cells and two regular cells removed"
    );
    // txs_count is not tracked by inline rollback deltas; the rebuild path
    // recomputes it from addr_txs which is not populated in this test.

    // script_info for lock_code_hash: only 2 regular cells remain as lock
    let lock_si = store.get_script_info(&lock_code_hash).unwrap().unwrap();
    assert_eq!(
        lock_si.lock_live_cells_count, 2,
        "lock script_info: 6 - 4 = 2 live cells"
    );
    assert_eq!(lock_si.lock_cells_count, 2);
    assert_eq!(lock_si.lock_capacity_sum, 2 * reg_cap);
    assert_eq!(lock_si.lock_used_capacity_sum, 2 * reg_occ);
    assert_eq!(
        lock_si.lock_owned_capacity_sum,
        2 * reg_cap,
        "lock script_info: capacity of 2 regular cells"
    );

    // script_info for type_code_hash: the script was first seen in a
    // rolled-back block, so its row is gone, as in a store that never saw
    // blocks 3-4 (B-0 finding 3: the reverse-counted rollback left an
    // all-zero row behind, which this test used to pin).
    assert!(
        store.get_script_info(&type_code_hash).unwrap().is_none(),
        "type script_info first written at block 3 must be gone"
    );

    // token_holder: balance reached 0, should be deleted
    let holder = store
        .get_token_holder_balance(&type_script_hash, &lock_hash)
        .unwrap();
    assert!(
        holder.is_none(),
        "token_holder should be deleted when balance reaches 0"
    );

    // Supply and holder count have one authoritative calculation path: token_holders.
    assert_eq!(
        store
            .aggregate_token_holder_stats(&type_script_hash)
            .unwrap(),
        (0, TokenBalance::zero())
    );
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
    journaled_headers(&domain, 1..=6);
    seed_epoch_rows(&domain, 1..=6i64);

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

    // Each channel opens in its block's journaled scope.
    commit_journaled_block(&domain, 3, |batch| {
        batch.put_fiber_channel(&channel_id_a, &channel_a);
        batch.put_addr_fiber_channel(&participant_a, &channel_id_a);
    })
    .unwrap();
    commit_journaled_block(&domain, 5, |batch| {
        batch.put_fiber_channel(&channel_id_b, &channel_b);
        batch.put_addr_fiber_channel(&participant_b, &channel_id_b);
    })
    .unwrap();

    // Sanity: both channels exist
    assert!(domain.get_fiber_channel(&channel_id_a).unwrap().is_some());
    assert!(domain.get_fiber_channel(&channel_id_b).unwrap().is_some());

    // Rollback to block 4
    rollback_fixture(&domain, 4, Some(&append)).unwrap();

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
    journaled_headers(&domain, 1..=6);
    seed_epoch_rows(&domain, 1..=6i64);

    // Channel opened at block 2, cooperatively closed at block 5, each state
    // in its block's journaled scope.
    let funding_tx = vec![0x03; 32];
    let channel_id = ckbadger_store::keys::encode_fiber_channel_id(&funding_tx, 0);
    let open = make_fiber_channel(
        &funding_tx,
        0,
        FiberChannelState::Open,
        400_00000000,
        2,
        vec![participant.clone()],
        vec![0xFC; 20],
    );
    let mut channel = open.clone();
    channel.state = FiberChannelState::CooperativelyClosed;
    channel.close_tx_hash = Some(vec![0xCC; 32]);
    channel.close_block = Some(5);
    channel.close_timestamp = Some(5000);
    channel.commitment_tx_hash = Some(vec![0xDD; 32]);
    channel.commitment_output_index = Some(0);

    commit_journaled_block(&domain, 2, |batch| {
        batch.put_fiber_channel(&channel_id, &open);
        batch.put_addr_fiber_channel(&participant, &channel_id);
    })
    .unwrap();
    commit_journaled_block(&domain, 5, |batch| {
        batch.put_fiber_channel(&channel_id, &channel);
        // Write commitment index
        batch.put_fiber_channel_by_commitment(
            channel.commitment_tx_hash.as_ref().unwrap(),
            &channel_id,
        );
    })
    .unwrap();

    // Rollback to block 4 — channel was opened at 2, closed at 5
    rollback_fixture(&domain, 4, Some(&append)).unwrap();

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

    let lock_args = vec![0x11; 32];
    let lock_code_hash = vec![0xAA; 32];

    // Insert blocks 1..5 with cells using lock_code_hash
    for b in 1..=5 {
        insert_full_block(&domain, Some(&append), b, &lock_args);
    }
    populate_derived_cfs(&domain, &lock_args, 5);

    // Pre-populate script_reference_info for the lock code_hash.
    // hash_type=1, is_type=false (lock side).
    // Simulate 5 cells using this script as lock: 5 total, 5 live.
    let cap_per_cell: i128 = 10_000_000_000;
    let occupied_per_cell = i128::from(make_cell(0, &lock_args).occupied_capacity());
    let sri = ScriptReferenceInfo {
        reference_hash: lock_code_hash.clone(),
        hash_type: 1,
        lock_cells_count: 5,
        lock_live_cells_count: 5,
        lock_capacity_sum: 5 * cap_per_cell,
        lock_owned_capacity_sum: 5 * cap_per_cell,
        lock_used_capacity_sum: 5 * occupied_per_cell,
        lock_owned_knowledge_sum: 5 * occupied_per_cell,
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
    let result = rollback_fixture(&domain, 2, Some(&append)).unwrap();
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
    assert_eq!(
        updated.lock_owned_knowledge_sum,
        2 * occupied_per_cell,
        "lock_owned_knowledge_sum: 2 surviving cells"
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
    journaled_headers(&domain, 1..=5);
    seed_epoch_rows(&domain, 1..=5i64);

    // Create activities: block 2 has single-participant, blocks 3 and 4 have
    // multi-participant activities (both lock_a and lock_b), each block's rows
    // in its journaled scope as the live writer writes them.

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
            id: ParticipantId::lock(&lock_a).unwrap(),
            ckb_delta: -5_000_000_000,
            used_delta: 0,
            item_deltas: vec![],
            tags: 0,
            roles: 0,
        }],
    };
    commit_journaled_block(&domain, 2, |batch| {
        batch.put_tx_actions(&actions_2);
        batch.put_addr_tx(&lock_a, 2, 0, &AddrTxValue::new(0, false, true, 0));
        index_tx(batch, 2, 0, &tx_hash_2);
    })
    .unwrap();

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
                id: ParticipantId::lock(&lock_a).unwrap(),
                ckb_delta: -10_000_000_000,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
                roles: 0,
            },
            ParticipantDelta {
                id: ParticipantId::lock(&lock_b).unwrap(),
                ckb_delta: 10_000_000_000,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
                roles: 0,
            },
        ],
    };
    commit_journaled_block(&domain, 3, |batch| {
        batch.put_tx_actions(&actions_3);
        batch.put_addr_tx(&lock_a, 3, 0, &AddrTxValue::new(0, false, true, 0));
        batch.put_addr_tx(&lock_b, 3, 0, &AddrTxValue::new(0, false, true, 0));
        index_tx(batch, 3, 0, &tx_hash_3);
    })
    .unwrap();

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
                id: ParticipantId::lock(&lock_a).unwrap(),
                ckb_delta: 5_000_000_000,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
                roles: 0,
            },
            ParticipantDelta {
                id: ParticipantId::lock(&lock_b).unwrap(),
                ckb_delta: -5_000_000_000,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
                roles: 0,
            },
        ],
    };
    commit_journaled_block(&domain, 4, |batch| {
        batch.put_tx_actions(&actions_4);
        batch.put_addr_tx(&lock_a, 4, 1, &AddrTxValue::new(0, false, true, 0));
        batch.put_addr_tx(&lock_b, 4, 1, &AddrTxValue::new(0, false, true, 0));
        index_tx(batch, 4, 1, &tx_hash_4);
    })
    .unwrap();

    // Sanity: global activity list has 3 entries
    let all = domain.list_tx_actions_recent(10, None).unwrap();
    assert_eq!(all.len(), 3, "3 TxActions written");

    // Sanity: lock_a appears in all 3, lock_b in 2
    let acts_a = domain.list_activities(&lock_a, 10, None, None).unwrap();
    assert_eq!(acts_a.len(), 3, "lock_a participates in all 3");
    let acts_b = domain.list_activities(&lock_b, 10, None, None).unwrap();
    assert_eq!(acts_b.len(), 2, "lock_b participates in blocks 3 and 4");

    // Rollback to block 2 — removes blocks 3,4,5
    rollback_fixture(&domain, 2, Some(&append)).unwrap();

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

/// The DAO singleton aggregates (`dao_latest_stats`, `dao_top_depositors`) are
/// tip-scoped derived rows that the indexer rewrites unconditionally right
/// after a rollback commits. Deleting them inside the rollback batch opens a
/// window in which the read path finds no singleton at all — an absence the API
/// cannot tell apart from "never written", which is how `/dao/top-depositors`
/// ended up serving an empty leaderboard for ~35-40s after every reorg.
/// Rollback must leave the previous row in place for the refresh to overwrite.
#[test]
fn test_rollback_preserves_dao_singleton_aggregates() {
    use ckbadger_store::types::{DaoLatestStatistics, DaoTopDepositorEntry, DaoTopDepositors};

    let (store, append_store) = setup_split_stores();
    let lock_args = vec![0xF1; 32];
    let lock_hash = owner_lock_hash(&lock_args);
    for i in 1..=10 {
        insert_full_block(&store, Some(append_store.as_ref()), i, &lock_args);
    }
    populate_derived_cfs(&store, &lock_args, 10);

    store
        .put_dao_top_depositors(&DaoTopDepositors {
            tip_block_number: 10,
            depositors: vec![DaoTopDepositorEntry {
                lock_script_hash: lock_hash.clone(),
                total_capacity: 10_200_000_000,
                deposit_count: 1,
                total_deposit_ms: 86_400_000,
            }],
        })
        .unwrap();

    let latest = DaoLatestStatistics {
        tip_block_number: 10,
        tip_block_hash: make_header(10).hash.try_into().unwrap(),
        total_deposited: 10_200_000_000,
        total_depositors: 1,
        active_deposits: 1,
        deposit_cells_count: 1,
        withdrawal_request_cells_count: 0,
        total_compensation_paid: 0,
        unclaimed_compensation: 0,
        average_deposit_days: Some("1 days".to_string()),
        estimated_apc: "2.00".to_string(),
        mining_reward: 0,
        deposit_compensation: 0,
        burnt: 0,
        pending_withdrawal_capacity: 0,
        // One 102-CKB-occupied deposit of 102 CKB: no free capacity.
        dao_capacity: 10_200_000_000,
        dao_free_capacity: 0,
        depositor_allocation_weight: 0,
        tip_total_issuance: 0,
        tip_secondary_pool: 0,
        tip_occupied_capacity: 0,
        rolling_24h: None,
    };
    let latest_key = ckbadger_store::keys::encode_stats_key(
        ckbadger_store::keys::STATS_PREFIX_DAO_LATEST_STATS,
        b"latest",
    );
    let mut batch = StoreBatch::new(&store);
    batch.put_stats(&latest_key, &bincode::serialize(&latest).unwrap());
    batch.commit().unwrap();

    rollback_fixture(&store, 5, Some(append_store.as_ref())).unwrap();

    assert!(
        store.get_dao_top_depositors().unwrap().is_some(),
        "rollback must not delete dao_top_depositors: the API cannot distinguish \
         the gap from a never-written singleton and serves an empty leaderboard"
    );
    assert!(
        store.get_latest_dao_statistics().unwrap().is_some(),
        "rollback must not delete dao_latest_stats: the post-rollback refresh \
         overwrites it in place"
    );
}

// ── Phase 1a: protocol-named participants ────────────────────────────────

/// The 20-byte prefix a protocol named in the rolled-back block.
const ROLLED_BACK_PREFIX: [u8; 20] = [0x77; 20];

fn prefix_participant_actions(
    block_num: i64,
    tx_idx: i32,
    tx_hash: &[u8],
    lock: &[u8],
) -> TxActions {
    TxActions {
        tx_hash: tx_hash.to_vec(),
        block_hash: make_header(block_num).hash,
        block_number: block_num,
        tx_index: tx_idx,
        timestamp: 1_000_000 + block_num * 1000,
        is_cellbase: false,
        protocol_actions: vec![],
        type_calls: vec![],
        lock_calls: vec![],
        participants: vec![
            ParticipantDelta {
                id: ParticipantId::lock(lock).unwrap(),
                ckb_delta: -1,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
                roles: 0,
            },
            ParticipantDelta {
                id: ParticipantId::LockPrefix(ROLLED_BACK_PREFIX),
                ckb_delta: 0,
                used_delta: 0,
                item_deltas: vec![],
                tags: ckbadger_store::types::TAG_IDENTITY,
                roles: ckbadger_store::types::participant_roles::OWNER_TO,
            },
        ],
    }
}

#[test]
fn test_rollback_deletes_prefix_participant_rows_and_reverses_prefix_stats() {
    let (domain, append) = setup_split_stores();
    let lock_a = vec![0xA0u8; 32];
    let tx_hash_2 = {
        let mut h = vec![0u8; 32];
        h[0] = 0x20;
        h
    };
    let tx_hash_3 = {
        let mut h = vec![0u8; 32];
        h[0] = 0x30;
        h
    };

    journaled_headers(&domain, 1..=3);
    seed_epoch_rows(&domain, 1..=3i64);

    // Both transactions sit at tx position 1: position 0 of a block is its
    // cellbase, which has no TxActions row. Each block's rows, and the two
    // counters as that block leaves them, in the block's journaled scope.
    for (block, tx_hash) in [(2i64, &tx_hash_2), (3, &tx_hash_3)] {
        commit_journaled_block(&domain, block, |batch| {
            batch.put_tx_actions(&prefix_participant_actions(block, 1, tx_hash, &lock_a));
            index_tx(batch, block, 1, tx_hash);
            batch.put_addr_tx(&lock_a, block, 1, &AddrTxValue::new(0, true, false, 0));
            batch.put_addr_tx_by_prefix(
                &ROLLED_BACK_PREFIX,
                block,
                1,
                &AddrTxValue::new(0, false, false, 0),
            );
            batch.put_addr_balance(
                &lock_a,
                &AddressBalance {
                    txs_count: block - 1,
                    ..Default::default()
                },
            );
            batch.put_addr_prefix_stats(
                &ROLLED_BACK_PREFIX,
                &ckbadger_store::types::AddrPrefixStats {
                    txs_count: block - 1,
                },
            );
        })
        .unwrap();
    }

    rollback_fixture(&domain, 2, Some(&append)).unwrap();

    // Block 3's rows are gone from both indexes; block 2's survive.
    let rows = domain.list_addr_txs_recent(&lock_a, 10, None).unwrap();
    let positions: Vec<(i64, i32)> = rows.iter().map(|(b, t, _, _)| (*b, *t)).collect();
    assert_eq!(
        positions,
        vec![(2, 1)],
        "only block 2 rows survive: {rows:?}"
    );
    let prefix_rows = domain
        .list_addr_txs_by_prefix_recent(&ROLLED_BACK_PREFIX, 10, None)
        .unwrap();
    assert_eq!(
        prefix_rows
            .iter()
            .map(|(b, t, _, _)| (*b, *t))
            .collect::<Vec<_>>(),
        vec![(2, 1)],
        "the rolled-back prefix row must be deleted"
    );

    // The counters end at the value block 2 left: equal to the rows that
    // survive.
    assert_eq!(
        domain
            .get_addr_prefix_stats(&ROLLED_BACK_PREFIX)
            .unwrap()
            .unwrap()
            .txs_count,
        1
    );
    assert_eq!(
        domain.get_addr_balance(&lock_a).unwrap().unwrap().txs_count,
        1
    );
}
