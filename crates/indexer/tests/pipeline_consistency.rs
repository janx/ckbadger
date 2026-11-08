//! BatchWriter Integration Tests
//!
//! Verifies that BatchWriter cell insertion, consumption, address balance
//! updates, and script usage tracking produce correct database state.

#![allow(clippy::type_complexity)]

use ckbadger_indexer::db::BatchWriter;
use ckbadger_indexer::parser::cell::ParsedCell;
use ckbadger_indexer::parser::udt::SUDT_CODE_HASH;
use ckbadger_indexer::parser::{CellParser, ScriptParser};
use ckbadger_indexer::rpc::{
    BlockResponseWithCycles, BlockView, CellInput, CellOutput, HeaderView, OutPoint, Script,
    TransactionView,
};
use ckbadger_indexer::sync::build_facts_arena_snapshot_for_test;
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::LiveCellInfo;
use ckbadger_store::CkbadgerStore;
use ckbadger_store::PositionedCellInfo;
use std::collections::HashMap;
use std::sync::Arc;

/// Real mainnet cellbase first witness (block 12,000,000): block parsing
/// requires every non-genesis cellbase to carry a valid RFC-0022
/// `CellbaseWitness`.
const TEST_CELLBASE_WITNESS: &str = "0x7a0000000c00000055000000490000001000000030000000310000009bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce801140000008211f1b938a107cd53b6302cc752a6fc3965638d210000000000000020302e3131332e3020283832383731613320323032342d30312d303929";

/// `lock_seed` fills the lock args, so each seed is a distinct owner; the
/// script hashes are the parser's, computed from the script fields.
fn make_cell(capacity: i64, data_size: i32, lock_seed: u8) -> ParsedCell {
    ParsedCell {
        capacity,
        lock_code_hash: vec![0x11u8; 32],
        lock_hash_type: 0,
        lock_args: vec![lock_seed; 20],
        lock_script_hash: ScriptParser::compute_script_hash_raw(&[0x11; 32], 0, &[lock_seed; 20]),
        type_code_hash: Some(vec![0x44u8; 32]),
        type_hash_type: Some(1),
        type_args: Some(vec![0x55u8; 20]),
        type_script_hash: Some(ScriptParser::compute_script_hash_raw(
            &[0x44; 32],
            1,
            &[0x55; 20],
        )),
        data_hash: [0x77u8; 32],
        data_size,
        data: vec![0u8; data_size as usize],
    }
}

fn setup_store() -> (Arc<CkbadgerStore>, BatchWriter) {
    let store = Arc::new(CkbadgerStore::open_test_unified_temp().unwrap());
    let writer = BatchWriter::new(store.clone(), store.clone());
    (store, writer)
}

fn occupied_capacity_from_cell(cell: &ParsedCell) -> i64 {
    let lock_script_size = 32 + 1 + cell.lock_args.len() as i64;
    let type_script_size = cell
        .type_args
        .as_ref()
        .map(|args| 32 + 1 + args.len() as i64)
        .unwrap_or(0);
    (8 + lock_script_size + type_script_size + i64::from(cell.data_size)) * 100_000_000
}

fn facts_fixture_lock_script() -> Script {
    Script {
        code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8".to_string(),
        hash_type: "type".to_string(),
        args: "0x927f3e74dceb87c81ba65a19da4f098b4de75a0d".to_string(),
    }
}

fn facts_fixture_header(number: u64) -> HeaderView {
    HeaderView {
        version: "0x0".to_string(),
        compact_target: "0x1a08a97e".to_string(),
        timestamp: "0x18c7b3b2b00".to_string(),
        number: format!("0x{number:x}"),
        epoch: "0x7080006000028".to_string(),
        parent_hash: format!("0x{}", "11".repeat(32)),
        transactions_root: format!("0x{}", "22".repeat(32)),
        proposals_hash: format!("0x{}", "33".repeat(32)),
        extra_hash: format!("0x{}", "44".repeat(32)),
        dao: format!("0x{}", "00".repeat(32)),
        nonce: "0x1".to_string(),
        hash: format!("0x{}", "55".repeat(32)),
    }
}

fn facts_fixture_block() -> BlockResponseWithCycles {
    let tx0 = TransactionView {
        hash: format!("0x{}", "aa".repeat(32)),
        version: "0x0".to_string(),
        cell_deps: vec![],
        header_deps: vec![],
        inputs: vec![CellInput {
            since: "0x0".to_string(),
            previous_output: OutPoint {
                tx_hash: format!("0x{}", "00".repeat(32)),
                index: "0xffffffff".to_string(),
            },
        }],
        outputs: vec![CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: facts_fixture_lock_script(),
            type_: None,
        }],
        outputs_data: vec!["0x".to_string()],
        witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
    };

    let tx1 = TransactionView {
        hash: format!("0x{}", "bb".repeat(32)),
        version: "0x0".to_string(),
        cell_deps: vec![],
        header_deps: vec![],
        inputs: vec![CellInput {
            since: "0x0".to_string(),
            previous_output: OutPoint {
                tx_hash: format!("0x{}", "cc".repeat(32)),
                index: "0x0".to_string(),
            },
        }],
        outputs: vec![CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: facts_fixture_lock_script(),
            type_: Some(Script {
                code_hash: SUDT_CODE_HASH.to_string(),
                hash_type: "type".to_string(),
                args: format!("0x{}", "12".repeat(32)),
            }),
        }],
        outputs_data: vec![format!("0x{}", hex::encode(42u128.to_le_bytes()))],
        witnesses: vec!["0x".to_string()],
    };

    BlockResponseWithCycles {
        block: BlockView {
            header: facts_fixture_header(14_000_123),
            uncles: vec![],
            transactions: vec![tx0, tx1],
            proposals: vec![],
        },
        cycles: None,
    }
}

fn precomputed_infos_for_insert(
    cells: &[(&[u8], i16, &ParsedCell, i64)],
) -> HashMap<(Vec<u8>, i16), PositionedCellInfo> {
    cells
        .iter()
        .map(|(tx_hash, output_index, cell, created_at_block)| {
            (
                ((*tx_hash).to_vec(), *output_index),
                PositionedCellInfo::new(cell.to_live_cell_info(None).unwrap(), *created_at_block),
            )
        })
        .collect()
}

fn insert_cells_for_test(
    store: &Arc<CkbadgerStore>,
    writer: &BatchWriter,
    cells: &[(&[u8], i16, &ParsedCell, i64)],
    skip_cell_indices: bool,
) {
    let precomputed = precomputed_infos_for_insert(cells);
    let mut domain_batch = StoreBatch::new(store);
    let mut cells_batch = StoreBatch::new(store);
    writer
        .insert_cells_batch(
            cells,
            &precomputed,
            &mut domain_batch,
            &mut cells_batch,
            skip_cell_indices,
        )
        .unwrap();
    cells_batch.commit().unwrap();
    domain_batch.commit().unwrap();
}

#[test]
fn test_cell_info_lookup_returns_all_fields() {
    let (store, writer) = setup_store();
    let tx_hash = vec![0x01u8; 32];
    let cell = make_cell(500_00000000, 256, 0xAA);

    insert_cells_for_test(&store, &writer, &[(&tx_hash, 0, &cell, 1000)], false);

    let result = writer.get_full_cells_info_batch(&[(&tx_hash, 0)]).unwrap();

    assert_eq!(result.len(), 1);
    let info = result.get(&(tx_hash.clone(), 0)).unwrap();

    assert_eq!(info.capacity, 500_00000000);
    assert_eq!(info.created_at_block, 1000);
    assert_eq!(info.lock_script_hash().to_vec(), cell.lock_script_hash);
    assert_eq!(info.data_size, 256);
}

#[test]
fn test_cell_info_batch_lookup_multiple_cells() {
    let (store, writer) = setup_store();

    let tx1 = vec![0x01u8; 32];
    let tx2 = vec![0x02u8; 32];
    let tx3 = vec![0x03u8; 32];

    let cell1 = make_cell(300_00000000, 100, 0xAA);
    let cell2 = make_cell(400_00000000, 200, 0xBB);
    let cell3 = make_cell(500_00000000, 300, 0xCC);

    insert_cells_for_test(
        &store,
        &writer,
        &[
            (&tx1, 0, &cell1, 1000),
            (&tx2, 0, &cell2, 2000),
            (&tx3, 0, &cell3, 3000),
        ],
        false,
    );

    let result = writer
        .get_full_cells_info_batch(&[(&tx1, 0), (&tx2, 0), (&tx3, 0)])
        .unwrap();

    assert_eq!(result.len(), 3);

    let info1 = result.get(&(tx1.clone(), 0)).unwrap();
    assert_eq!(info1.capacity, 300_00000000);
    assert_eq!(info1.created_at_block, 1000);
    assert_eq!(info1.data_size, 100);

    let info2 = result.get(&(tx2.clone(), 0)).unwrap();
    assert_eq!(info2.capacity, 400_00000000);
    assert_eq!(info2.created_at_block, 2000);
    assert_eq!(info2.data_size, 200);

    let info3 = result.get(&(tx3.clone(), 0)).unwrap();
    assert_eq!(info3.capacity, 500_00000000);
    assert_eq!(info3.created_at_block, 3000);
    assert_eq!(info3.data_size, 300);
}

#[test]
fn test_chunked_batch_lookup_matches_single_call_across_chunk_boundary() {
    // Confirms get_full_cells_info_batch_chunk returns identical results
    // whether the caller issues one big call or N smaller chunks. This is
    // the contract the parser's chunked retry loop relies on.
    let (store, writer) = setup_store();

    // Use 1500 cells so we cross the parser's 512-key chunk boundary
    // multiple times.
    let n: u32 = 1500;
    let tx_hashes: Vec<Vec<u8>> = (0..n)
        .map(|i| {
            let mut h = vec![0u8; 32];
            h[0..4].copy_from_slice(&i.to_be_bytes());
            h
        })
        .collect();
    // Capacity must cover occupied = (8 + lock(33+20) + type(33+20) + data 64) * 1e8
    // ≈ 178 CKB; use 500 CKB base so all values clear the threshold.
    let cells: Vec<ParsedCell> = (0..n)
        .map(|i| make_cell(500_00000000 + i64::from(i), 64, (i & 0xff) as u8))
        .collect();
    let inserts: Vec<(&[u8], i16, &ParsedCell, i64)> = (0..n as usize)
        .map(|i| (tx_hashes[i].as_slice(), 0i16, &cells[i], 100 + i as i64))
        .collect();
    insert_cells_for_test(&store, &writer, &inserts, false);

    let outpoints: Vec<(&[u8], i16)> = tx_hashes.iter().map(|h| (h.as_slice(), 0i16)).collect();

    // 1) Single call.
    let single = writer.get_full_cells_info_batch(&outpoints).unwrap();
    assert_eq!(single.len(), n as usize);

    // 2) Chunked: 512 keys per chunk (matches parser's PARSER_CELL_LOOKUP_CHUNK_SIZE).
    let mut chunked: HashMap<(Vec<u8>, i16), PositionedCellInfo> =
        HashMap::with_capacity(n as usize);
    for chunk in outpoints.chunks(512) {
        let part = writer.get_full_cells_info_batch_chunk(chunk).unwrap();
        chunked.extend(part);
    }
    assert_eq!(chunked.len(), single.len());

    // 3) Per-key equality: capacity, created_at_block, hashes match.
    for k in single.keys() {
        let s = single.get(k).unwrap();
        let c = chunked.get(k).expect("chunked must have every key");
        assert_eq!(s.capacity, c.capacity, "capacity mismatch for {:?}", k);
        assert_eq!(
            s.created_at_block, c.created_at_block,
            "created_at_block mismatch for {:?}",
            k
        );
        assert_eq!(s.lock_script_hash(), c.lock_script_hash());
        assert_eq!(s.data_size, c.data_size);
    }
}

#[test]
fn test_full_cells_info_returns_lock_and_type() {
    let (store, writer) = setup_store();
    let tx_hash = vec![0x01u8; 32];
    let cell = ParsedCell {
        capacity: 300_00000000,
        lock_code_hash: vec![0x11u8; 32],
        lock_hash_type: 0,
        lock_args: vec![0x22u8; 20],
        lock_script_hash: ScriptParser::compute_script_hash_raw(&[0x11; 32], 0, &[0x22; 20]),
        type_code_hash: Some(vec![0x44u8; 32]),
        type_hash_type: Some(1),
        type_args: Some(vec![0x55u8; 20]),
        type_script_hash: Some(ScriptParser::compute_script_hash_raw(
            &[0x44; 32],
            1,
            &[0x55; 20],
        )),
        data_hash: [0x77u8; 32],
        data_size: 100,
        data: vec![0u8; 100],
    };

    insert_cells_for_test(&store, &writer, &[(&tx_hash, 0, &cell, 1000)], false);

    let result = writer.get_full_cells_info_batch(&[(&tx_hash, 0)]).unwrap();

    assert_eq!(result.len(), 1);
    let info = result.get(&(tx_hash.clone(), 0)).unwrap();

    assert_eq!(info.lock_code_hash, [0x11u8; 32]);
    assert_eq!(info.lock_hash_type, 0);
    assert_eq!(info.type_code_hash, Some([0x44u8; 32]));
    assert_eq!(info.type_hash_type, Some(1));
    assert_eq!(info.capacity, 300_00000000);
    assert_eq!(info.created_at_block, 1000);
}

#[test]
fn test_full_cells_info_no_type_script() {
    let (store, writer) = setup_store();
    let tx_hash = vec![0x01u8; 32];
    let cell = ParsedCell {
        capacity: 300_00000000,
        lock_code_hash: vec![0x11u8; 32],
        lock_hash_type: 0,
        lock_args: vec![0x22u8; 20],
        lock_script_hash: ScriptParser::compute_script_hash_raw(&[0x11; 32], 0, &[0x22; 20]),
        type_code_hash: None,
        type_hash_type: None,
        type_args: None,
        type_script_hash: None,
        data_hash: [0x77u8; 32],
        data_size: 100,
        data: vec![0u8; 100],
    };

    insert_cells_for_test(&store, &writer, &[(&tx_hash, 0, &cell, 1000)], false);

    let result = writer.get_full_cells_info_batch(&[(&tx_hash, 0)]).unwrap();

    assert_eq!(result.len(), 1);
    let info = result.get(&(tx_hash.clone(), 0)).unwrap();

    assert_eq!(info.lock_code_hash, [0x11u8; 32]);
    assert_eq!(info.type_code_hash, None);
}

#[test]
fn test_full_cells_info_errors_when_capacity_is_below_the_derived_occupied_capacity() {
    // The payload stores no occupied capacity; the writer derives it (here
    // 8 + 33 + 20 + 33 + 20 + 100 bytes = 214 CKB) and refuses a stored cell
    // whose capacity cannot hold its own fields.
    let (store, writer) = setup_store();
    let tx_hash = vec![0x31u8; 32];

    let underfunded = LiveCellInfo {
        capacity: 213_00000000,
        lock_code_hash: [0x11u8; 32],
        lock_hash_type: 0,
        lock_args: vec![0x22u8; 20],
        type_code_hash: Some([0x44u8; 32]),
        type_hash_type: Some(0),
        type_args: Some(vec![0x55u8; 20]),
        data_size: 100,
        udt_amount: None,
        data_hash: None,
    };

    let mut batch = StoreBatch::new(&store);
    batch.put_cell(&tx_hash, 0, &underfunded, 1000);
    batch.commit().unwrap();

    let err = writer
        .get_full_cells_info_batch(&[(&tx_hash, 0)])
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("invalid occupied capacity")
            && err.contains("occupied=21400000000")
            && err.contains("capacity=21300000000"),
        "{err}"
    );
}

#[test]
fn test_typed_cell_lacking_type_args_is_never_stored() {
    let (store, _writer) = setup_store();
    let tx_hash = vec![0x41u8; 32];

    let bad = LiveCellInfo {
        capacity: 300_00000000,
        lock_code_hash: [0x11u8; 32],
        lock_hash_type: 0,
        lock_args: vec![0x22u8; 20],
        type_code_hash: Some([0x44u8; 32]),
        type_hash_type: Some(0),
        type_args: None,
        data_size: 100,
        udt_amount: None,
        data_hash: None,
    };

    // The v2 payload holds a whole type script or none: a typed payload
    // without args is refused when it is written, with its outpoint (and when
    // read: `ckbadger-store` `types::tests`), so no reader is handed one.
    let mut batch = StoreBatch::new(&store);
    let err = batch
        .put_cell_payload_by_outpoint(&tx_hash, 0, &bad)
        .unwrap_err();
    assert!(err.to_string().contains("type script is partial"), "{err}");
    assert!(err.to_string().contains(&hex::encode(&tx_hash)), "{err}");
    assert!(batch.is_empty(), "nothing staged");
}

#[test]
fn test_script_usage_cell_creation() {
    let (store, writer) = setup_store();
    let lock_code_hash = vec![0x11u8; 32];
    let mut changes: HashMap<(Vec<u8>, bool), (i64, i64, i128, i128, i128, i128)> = HashMap::new();

    changes.insert(
        (lock_code_hash.clone(), false),
        (1, 1, 100_00000000, 100_00000000, 61_00000000, 61_00000000),
    );

    let mut batch = StoreBatch::new(&store);
    writer
        .update_script_usage_batch(&changes, &mut batch)
        .unwrap();
    batch.commit().unwrap();

    // Verify via ScriptInfo in store
    let info = store.get_script_info(&lock_code_hash).unwrap();
    assert!(info.is_some());
    let info = info.unwrap();
    assert_eq!(info.lock_cells_count, 1);
    assert_eq!(info.lock_live_cells_count, 1);
    assert_eq!(info.lock_capacity_sum, 100_00000000);
    assert_eq!(info.lock_owned_capacity_sum, 100_00000000);
    assert_eq!(info.lock_used_capacity_sum, 61_00000000);
    assert_eq!(info.lock_owned_knowledge_sum, 61_00000000);
}

#[test]
fn test_script_usage_cell_consumption() {
    let (store, writer) = setup_store();
    let lock_code_hash = vec![0x11u8; 32];

    let mut create_changes: HashMap<(Vec<u8>, bool), (i64, i64, i128, i128, i128, i128)> =
        HashMap::new();
    create_changes.insert(
        (lock_code_hash.clone(), false),
        (1, 1, 100_00000000, 100_00000000, 61_00000000, 61_00000000),
    );
    let mut batch = StoreBatch::new(&store);
    writer
        .update_script_usage_batch(&create_changes, &mut batch)
        .unwrap();
    batch.commit().unwrap();

    let mut consume_changes: HashMap<(Vec<u8>, bool), (i64, i64, i128, i128, i128, i128)> =
        HashMap::new();
    consume_changes.insert(
        (lock_code_hash.clone(), false),
        (0, -1, 0, -100_00000000, 0, -61_00000000),
    );
    let mut batch = StoreBatch::new(&store);
    writer
        .update_script_usage_batch(&consume_changes, &mut batch)
        .unwrap();
    batch.commit().unwrap();

    let info = store.get_script_info(&lock_code_hash).unwrap();
    let info = info.unwrap();
    assert_eq!(info.lock_cells_count, 1);
    assert_eq!(info.lock_live_cells_count, 0);
    assert_eq!(info.lock_capacity_sum, 100_00000000);
    assert_eq!(info.lock_owned_capacity_sum, 0);
    assert_eq!(info.lock_used_capacity_sum, 61_00000000);
    assert_eq!(info.lock_owned_knowledge_sum, 0);
}

fn make_addr_delta(
    balance_delta: i128,
    live_delta: i32,
    total_delta: i32,
    tx_delta: i64,
    block_num: i64,
    tx_idx: i32,
    used_delta: i128,
) -> ckbadger_indexer::sync::types::AddressBalanceDelta {
    ckbadger_indexer::sync::types::AddressBalanceDelta {
        balance_delta,
        live_delta,
        total_delta,
        tx_delta,
        cellbase_tx_delta: 0,
        used_delta,
        first_seen_block: block_num,
        first_seen_tx_idx: tx_idx,
        last_activity_block: block_num,
        last_activity_tx_idx: tx_idx,
    }
}

fn apply_addr_changes(
    store: &CkbadgerStore,
    writer: &ckbadger_indexer::db::BatchWriter,
    changes: &HashMap<Vec<u8>, ckbadger_indexer::sync::types::AddressBalanceDelta>,
) -> anyhow::Result<()> {
    let keys: Vec<&Vec<u8>> = changes.keys().collect();
    let existing = writer.read_address_balances(&keys)?;
    let mut batch = StoreBatch::new(store);
    writer.apply_address_balance_deltas(&existing, changes, &mut batch)?;
    batch.commit()?;
    Ok(())
}

#[test]
fn test_address_balance_update_receive() {
    let (store, writer) = setup_store();
    let lock_hash = vec![0xAAu8; 32];

    let changes: HashMap<Vec<u8>, _> = [(
        lock_hash.clone(),
        make_addr_delta(100_00000000, 1, 1, 1, 1000, 1, 0),
    )]
    .into_iter()
    .collect();

    apply_addr_changes(&store, &writer, &changes).unwrap();

    let balance = store.get_addr_balance(&lock_hash).unwrap();
    assert!(balance.is_some());
    let balance = balance.unwrap();
    assert_eq!(balance.balance, 100_00000000);
    assert_eq!(balance.live_cells_count, 1);
    assert_eq!(balance.txs_count, 1);
}

#[test]
fn test_address_balance_update_send() {
    let (store, writer) = setup_store();
    let lock_hash = vec![0xAAu8; 32];

    let receive: HashMap<Vec<u8>, _> = [(
        lock_hash.clone(),
        make_addr_delta(100_00000000, 1, 1, 1, 1000, 1, 0),
    )]
    .into_iter()
    .collect();
    apply_addr_changes(&store, &writer, &receive).unwrap();

    let send: HashMap<Vec<u8>, _> = [(
        lock_hash.clone(),
        make_addr_delta(-30_00000000, 0, 1, 1, 2000, 2, 0),
    )]
    .into_iter()
    .collect();
    apply_addr_changes(&store, &writer, &send).unwrap();

    let balance = store.get_addr_balance(&lock_hash).unwrap();
    let balance = balance.unwrap();
    assert_eq!(balance.balance, 70_00000000);
    assert_eq!(balance.live_cells_count, 1);
    assert_eq!(balance.txs_count, 2);
}

#[test]
fn test_address_balance_used_delta_applied() {
    let (store, writer) = setup_store();
    let lock_hash = vec![0xBBu8; 32];

    // Receive: creates a cell with 6100 CKB occupied
    let receive: HashMap<Vec<u8>, _> = [(
        lock_hash.clone(),
        make_addr_delta(100_00000000, 1, 1, 1, 1000, 1, 6100_00000000),
    )]
    .into_iter()
    .collect();
    apply_addr_changes(&store, &writer, &receive).unwrap();

    let balance = store.get_addr_balance(&lock_hash).unwrap().unwrap();
    assert_eq!(balance.used_capacity, 6100_00000000);

    // Consume old cell (-6100) and create new smaller cell (+4100)
    let update: HashMap<Vec<u8>, _> = [(
        lock_hash.clone(),
        make_addr_delta(0, 0, 1, 1, 2000, 2, -2000_00000000),
    )]
    .into_iter()
    .collect();
    apply_addr_changes(&store, &writer, &update).unwrap();

    let balance = store.get_addr_balance(&lock_hash).unwrap().unwrap();
    assert_eq!(balance.used_capacity, 4100_00000000);
}

#[test]
fn test_address_balance_used_underflow_errors() {
    let (store, writer) = setup_store();
    let lock_hash = vec![0xCCu8; 32];

    // Apply a negative used_delta larger than what exists (0)
    // Should fail fast instead of silently clamping.
    let changes: HashMap<Vec<u8>, _> = [(
        lock_hash.clone(),
        make_addr_delta(100_00000000, 1, 1, 1, 1000, 1, -9999_00000000),
    )]
    .into_iter()
    .collect();
    let err = apply_addr_changes(&store, &writer, &changes).unwrap_err();
    assert!(err.to_string().contains("underflow"));
}

#[test]
fn test_multiple_outputs_same_tx() {
    let (store, writer) = setup_store();
    let tx_hash = vec![0x01u8; 32];
    let cell0 = make_cell(300_00000000, 100, 0xAA);
    let cell1 = make_cell(400_00000000, 200, 0xBB);
    let cell2 = make_cell(500_00000000, 300, 0xCC);

    insert_cells_for_test(
        &store,
        &writer,
        &[
            (&tx_hash, 0, &cell0, 1000),
            (&tx_hash, 1, &cell1, 1000),
            (&tx_hash, 2, &cell2, 1000),
        ],
        false,
    );

    let result = writer
        .get_full_cells_info_batch(&[(&tx_hash, 0), (&tx_hash, 1), (&tx_hash, 2)])
        .unwrap();

    assert_eq!(result.len(), 3);

    let info0 = result.get(&(tx_hash.clone(), 0)).unwrap();
    assert_eq!(info0.capacity, 300_00000000);

    let info1 = result.get(&(tx_hash.clone(), 1)).unwrap();
    assert_eq!(info1.capacity, 400_00000000);

    let info2 = result.get(&(tx_hash.clone(), 2)).unwrap();
    assert_eq!(info2.capacity, 500_00000000);
}

#[test]
fn test_cell_lookup_across_height_ranges() {
    let (store, writer) = setup_store();
    let tx_low_height = vec![0x01u8; 32];
    let tx_high_height = vec![0x02u8; 32];
    let cell = make_cell(300_00000000, 100, 0xAA);

    insert_cells_for_test(
        &store,
        &writer,
        &[
            (&tx_low_height, 0, &cell, 1_000_000),
            (&tx_high_height, 0, &cell, 6_000_000),
        ],
        false,
    );

    let result = writer
        .get_full_cells_info_batch(&[(&tx_low_height, 0), (&tx_high_height, 0)])
        .unwrap();

    assert_eq!(result.len(), 2);

    let info0 = result.get(&(tx_low_height.clone(), 0)).unwrap();
    assert_eq!(info0.created_at_block, 1_000_000);

    let info1 = result.get(&(tx_high_height.clone(), 0)).unwrap();
    assert_eq!(info1.created_at_block, 6_000_000);
}

// --- Tests for skip_cell_indices ---

#[test]
fn test_skip_cell_indices_omits_index_entries() {
    let (store, writer) = setup_store();
    let tx_hash = vec![0x01u8; 32];
    let cell = make_cell(100_00000000, 256, 0xAA);

    // Insert with skip_cell_indices = true
    insert_cells_for_test(&store, &writer, &[(&tx_hash, 0, &cell, 1000)], true);

    // Live cell itself should exist
    let live = store.get_cell(&tx_hash, 0, &store).unwrap();
    assert!(live.is_some(), "cell should be in live_cells");

    // But index entries should NOT exist
    let by_lock = store
        .list_cells_by_lock(&cell.lock_script_hash, 100, None, &store)
        .unwrap();
    assert!(
        by_lock.is_empty(),
        "lock index should be empty when skipped"
    );

    let by_type = store
        .list_cells_by_type(cell.type_script_hash.as_ref().unwrap(), 100, None, &store)
        .unwrap();
    assert!(
        by_type.is_empty(),
        "type index should be empty when skipped"
    );

    let by_lock_code = store
        .list_cells_by_lock_code_hash(
            &cell.lock_code_hash,
            cell.lock_hash_type as u8,
            100,
            None,
            &store,
        )
        .unwrap();
    assert!(
        by_lock_code.is_empty(),
        "lock_code index should be empty when skipped"
    );

    let by_type_code = store
        .list_cells_by_type_code_hash(
            cell.type_code_hash.as_ref().unwrap(),
            cell.type_hash_type.unwrap() as u8,
            100,
            None,
            &store,
        )
        .unwrap();
    assert!(
        by_type_code.is_empty(),
        "type_code index should be empty when skipped"
    );
}

#[test]
fn test_live_sync_files_cell_by_code_rows_under_their_hash_type_form() {
    // The live-sync writer must file each cell-by-code row under the script's
    // own (code_hash, hash_type) form, so a query for a sibling form of the
    // same code hash sees nothing instead of scanning across forms.
    let (store, writer) = setup_store();
    let data_form_tx = vec![0x01u8; 32];
    let type_form_tx = vec![0x02u8; 32];

    // `make_cell` uses lock hash_type=0 (data) and type hash_type=1 (type).
    let data_form_cell = make_cell(200_00000000, 0, 0xAA);
    let mut type_form_cell = make_cell(300_00000000, 0, 0xBB);
    type_form_cell.lock_hash_type = 1;

    insert_cells_for_test(
        &store,
        &writer,
        &[
            (&data_form_tx, 0, &data_form_cell, 1000),
            (&type_form_tx, 0, &type_form_cell, 1001),
        ],
        false,
    );

    let code_hash = &data_form_cell.lock_code_hash;
    assert_eq!(code_hash, &type_form_cell.lock_code_hash);

    let data_form = store
        .list_cells_by_lock_code_hash(code_hash, 0, 100, None, &store)
        .unwrap();
    assert_eq!(data_form.len(), 1, "data form holds exactly its own cell");
    assert_eq!(data_form[0].0, data_form_tx);

    let type_form = store
        .list_cells_by_lock_code_hash(code_hash, 1, 100, None, &store)
        .unwrap();
    assert_eq!(type_form.len(), 1, "type form holds exactly its own cell");
    assert_eq!(type_form[0].0, type_form_tx);

    // An unobserved form under the same code hash is empty.
    assert!(store
        .list_cells_by_lock_code_hash(code_hash, 4, 100, None, &store)
        .unwrap()
        .is_empty());

    // Consumption removes the row from the same form it was filed under.
    let mut domain_batch = StoreBatch::new(&store);
    let preloaded = writer
        .get_full_cells_info_batch(&[(&data_form_tx, 0)])
        .unwrap();
    // What the parser ships with a live batch: each input's script hashes.
    let shipped: HashMap<(Vec<u8>, i16), ([u8; 32], Option<[u8; 32]>)> = preloaded
        .iter()
        .map(|(key, info)| {
            (
                key.clone(),
                (info.lock_script_hash(), info.type_script_hash()),
            )
        })
        .collect();
    writer
        .consume_cells_batch_preloaded(
            &[(&data_form_tx, 0, 1000, &type_form_tx, 1002, 0)],
            &preloaded,
            &Default::default(),
            &shipped,
            &mut domain_batch,
            false,
        )
        .unwrap();
    domain_batch.commit().unwrap();

    assert!(store
        .list_cells_by_lock_code_hash(code_hash, 0, 100, None, &store)
        .unwrap()
        .is_empty());
    assert_eq!(
        store
            .list_cells_by_lock_code_hash(code_hash, 1, 100, None, &store)
            .unwrap()
            .len(),
        1,
        "consuming a data-form cell must not touch the type form"
    );
}

#[test]
fn facts_arena_snapshot_matches_direct_cell_math() {
    let block = facts_fixture_block();
    let snapshot = build_facts_arena_snapshot_for_test(std::slice::from_ref(&block)).unwrap();

    let expected_occupied: Vec<i64> = block
        .block
        .transactions
        .iter()
        .flat_map(|tx| {
            CellParser::parse_outputs(tx)
                .unwrap()
                .into_iter()
                .map(|cell| occupied_capacity_from_cell(&cell))
                .collect::<Vec<i64>>()
        })
        .collect();
    let actual_occupied: Vec<i64> = snapshot
        .cells
        .iter()
        .map(|cell| cell.occupied_capacity)
        .collect();

    assert_eq!(snapshot.tx_count, 2);
    assert_eq!(actual_occupied, expected_occupied);
}
