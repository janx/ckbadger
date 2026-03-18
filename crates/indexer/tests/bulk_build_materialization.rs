use ckbadger_indexer::rpc::{
    BlockResponseWithCycles, BlockView, CellInput, CellOutput, HeaderView, OutPoint, Script,
    TransactionView,
};
use ckbadger_indexer::parser::ScriptParser;
use ckbadger_store::types::ConsumedCellMeta;
use ckbadger_indexer::sync::{
    materialize_bulk_artifacts_for_test, run_sample_bulk_materialization_for_test,
};
use ckbadger_store::keys;

fn fixture_lock_script() -> Script {
    Script {
        code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
            .to_string(),
        hash_type: "type".to_string(),
        args: "0x927f3e74dceb87c81ba65a19da4f098b4de75a0d".to_string(),
    }
}

fn fixture_header(number: u64, hash_byte: u8) -> HeaderView {
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
        hash: format!("0x{}", format!("{hash_byte:02x}").repeat(32)),
    }
}

fn same_block_create_then_consume_fixture() -> BlockResponseWithCycles {
    let create_tx = TransactionView {
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
            capacity: "0x2540be400".to_string(),
            lock: fixture_lock_script(),
            type_: None,
        }],
        outputs_data: vec!["0x".to_string()],
        witnesses: vec!["0x".to_string()],
    };

    let consume_tx = TransactionView {
        hash: format!("0x{}", "bb".repeat(32)),
        version: "0x0".to_string(),
        cell_deps: vec![],
        header_deps: vec![],
        inputs: vec![CellInput {
            since: "0x0".to_string(),
            previous_output: OutPoint {
                tx_hash: create_tx.hash.clone(),
                index: "0x0".to_string(),
            },
        }],
        outputs: vec![CellOutput {
            capacity: "0x2540be400".to_string(),
            lock: fixture_lock_script(),
            type_: None,
        }],
        outputs_data: vec!["0x".to_string()],
        witnesses: vec!["0x".to_string()],
    };

    BlockResponseWithCycles {
        block: BlockView {
            header: fixture_header(14_000_321, 0x55),
            uncles: vec![],
            transactions: vec![create_tx, consume_tx],
            proposals: vec![],
        },
        cycles: None,
    }
}

fn fixture_type_script() -> Script {
    Script {
        code_hash: format!("0x{}", "66".repeat(32)),
        hash_type: "type".to_string(),
        args: format!("0x{}", "77".repeat(20)),
    }
}

fn same_block_typed_data_create_then_consume_fixture() -> BlockResponseWithCycles {
    let create_tx = TransactionView {
        hash: format!("0x{}", "c1".repeat(32)),
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
            capacity: "0x4a817c800".to_string(),
            lock: fixture_lock_script(),
            type_: Some(fixture_type_script()),
        }],
        outputs_data: vec!["0xdeadbeef".to_string()],
        witnesses: vec!["0x".to_string()],
    };

    let consume_tx = TransactionView {
        hash: format!("0x{}", "d2".repeat(32)),
        version: "0x0".to_string(),
        cell_deps: vec![],
        header_deps: vec![],
        inputs: vec![CellInput {
            since: "0x0".to_string(),
            previous_output: OutPoint {
                tx_hash: create_tx.hash.clone(),
                index: "0x0".to_string(),
            },
        }],
        outputs: vec![CellOutput {
            capacity: "0x4a817c800".to_string(),
            lock: fixture_lock_script(),
            type_: Some(fixture_type_script()),
        }],
        outputs_data: vec!["0xdeadbeef".to_string()],
        witnesses: vec!["0x".to_string()],
    };

    BlockResponseWithCycles {
        block: BlockView {
            header: fixture_header(14_000_777, 0x88),
            uncles: vec![],
            transactions: vec![create_tx, consume_tx],
            proposals: vec![],
        },
        cycles: None,
    }
}

#[test]
fn materializer_streams_append_only_rows_before_final_snapshot() {
    let report = run_sample_bulk_materialization_for_test().expect("sample bulk materialization");

    assert!(report.streamed_history_rows > 0);
    assert!(report.final_snapshot_rows > 0);
}

#[test]
fn bulk_build_materializes_history_rows_and_core_snapshots_from_single_pass() {
    let block = same_block_create_then_consume_fixture();
    let lock_hash = ScriptParser::compute_script_hash(&fixture_lock_script());
    let block_hash = hex::decode(&block.block.header.hash[2..]).expect("block hash");
    let create_tx_hash = hex::decode(&block.block.transactions[0].hash[2..]).expect("create tx hash");
    let consume_tx_hash =
        hex::decode(&block.block.transactions[1].hash[2..]).expect("consume tx hash");

    let snapshot =
        materialize_bulk_artifacts_for_test(&[block]).expect("bulk build artifact snapshot");

    assert_eq!(snapshot.report.streamed_history_rows, 15);
    assert!(snapshot.report.final_snapshot_rows > 0);

    let header = snapshot
        .block_headers
        .get(&14_000_321)
        .expect("block header");
    assert_eq!(header.transactions_count, 2);
    assert_eq!(
        snapshot.block_numbers_by_hash.get(&block_hash),
        Some(&14_000_321)
    );

    let create_tx = snapshot.txs_by_hash.get(&create_tx_hash).expect("create tx");
    assert_eq!(create_tx.0, 14_000_321);
    assert_eq!(create_tx.1, 0);
    assert!(create_tx.2.is_cellbase);
    assert_eq!(create_tx.2.inputs_count, 1);
    assert_eq!(create_tx.2.outputs_count, 1);
    assert_eq!(create_tx.2.fee, 0);

    let consume_tx = snapshot
        .txs_by_hash
        .get(&consume_tx_hash)
        .expect("consume tx");
    assert_eq!(consume_tx.0, 14_000_321);
    assert_eq!(consume_tx.1, 1);
    assert!(!consume_tx.2.is_cellbase);
    assert_eq!(consume_tx.2.inputs_count, 1);
    assert_eq!(consume_tx.2.outputs_count, 1);
    assert_eq!(consume_tx.2.fee, 0);

    assert!(snapshot.core.address_balances.contains_key(lock_hash.as_slice()));
}

#[test]
fn bulk_build_materializes_append_only_cells_and_final_live_markers_from_single_pass() {
    let block = same_block_create_then_consume_fixture();
    let create_tx_hash = hex::decode(&block.block.transactions[0].hash[2..]).expect("create tx hash");
    let consume_tx_hash =
        hex::decode(&block.block.transactions[1].hash[2..]).expect("consume tx hash");
    let create_outpoint = keys::encode_outpoint(&create_tx_hash, 0).to_vec();
    let consume_outpoint = keys::encode_outpoint(&consume_tx_hash, 0).to_vec();

    let snapshot =
        materialize_bulk_artifacts_for_test(&[block]).expect("bulk build artifact snapshot");

    assert_eq!(snapshot.report.streamed_history_rows, 15);
    assert_eq!(snapshot.cell_payloads.len(), 2);
    assert!(snapshot.cell_payloads.contains_key(&create_outpoint));
    assert!(snapshot.cell_payloads.contains_key(&consume_outpoint));
    assert_eq!(snapshot.cell_payloads[&create_outpoint].capacity, 100_00000000);
    assert_eq!(snapshot.cell_payloads[&consume_outpoint].capacity, 100_00000000);

    assert_eq!(snapshot.live_cells.len(), 1);
    assert!(!snapshot.live_cells.contains_key(&create_outpoint));
    assert_eq!(snapshot.live_cells.get(&consume_outpoint), Some(&14_000_321));
}

#[test]
fn bulk_build_materializes_consumed_cells_and_live_cell_indexes_from_single_pass() {
    let block = same_block_typed_data_create_then_consume_fixture();
    let create_tx_hash = hex::decode(&block.block.transactions[0].hash[2..]).expect("create tx hash");
    let consume_tx_hash =
        hex::decode(&block.block.transactions[1].hash[2..]).expect("consume tx hash");
    let create_outpoint = keys::encode_outpoint(&create_tx_hash, 0).to_vec();
    let lock_hash = ScriptParser::compute_script_hash(&fixture_lock_script());
    let lock_code_hash = hex::decode(&fixture_lock_script().code_hash[2..]).expect("lock code hash");
    let type_hash = ScriptParser::compute_script_hash(&fixture_type_script());
    let type_code_hash = hex::decode(&fixture_type_script().code_hash[2..]).expect("type code hash");
    let data_hash = ScriptParser::compute_data_hash(&hex::decode("deadbeef").expect("output data"));

    let snapshot =
        materialize_bulk_artifacts_for_test(&[block]).expect("bulk build artifact snapshot");

    let consumed = snapshot
        .consumed_cells
        .get(&create_outpoint)
        .expect("consumed cell meta");
    assert_eq!(
        consumed,
        &ConsumedCellMeta {
            created_at_block: 14_000_777,
            consumed_at_block: 14_000_777,
            consumed_by_tx: Some(consume_tx_hash.clone()),
        }
    );

    let expected_live_lock = keys::encode_cell_index_key(&lock_hash, 14_000_777, &consume_tx_hash, 0);
    let expected_live_lock_code =
        keys::encode_cell_index_key(&lock_code_hash, 14_000_777, &consume_tx_hash, 0);
    let expected_live_type = keys::encode_cell_index_key(&type_hash, 14_000_777, &consume_tx_hash, 0);
    let expected_live_type_code =
        keys::encode_cell_index_key(&type_code_hash, 14_000_777, &consume_tx_hash, 0);
    let old_lock_key = keys::encode_cell_index_key(&lock_hash, 14_000_777, &create_tx_hash, 0);

    assert_eq!(snapshot.cell_by_lock.len(), 1);
    assert!(snapshot.cell_by_lock.contains(&expected_live_lock));
    assert!(!snapshot.cell_by_lock.contains(&old_lock_key));
    assert_eq!(snapshot.cell_by_lock_code.len(), 1);
    assert!(snapshot.cell_by_lock_code.contains(&expected_live_lock_code));
    assert_eq!(snapshot.cell_by_type.len(), 1);
    assert!(snapshot.cell_by_type.contains(&expected_live_type));
    assert_eq!(snapshot.cell_by_type_code.len(), 1);
    assert!(snapshot.cell_by_type_code.contains(&expected_live_type_code));

    let expected_data_hash_create =
        keys::encode_cell_index_key(&data_hash, 14_000_777, &create_tx_hash, 0);
    let expected_data_hash_consume =
        keys::encode_cell_index_key(&data_hash, 14_000_777, &consume_tx_hash, 0);
    assert_eq!(snapshot.cell_by_data_hash.len(), 2);
    assert!(snapshot.cell_by_data_hash.contains(&expected_data_hash_create));
    assert!(snapshot.cell_by_data_hash.contains(&expected_data_hash_consume));
}

#[test]
fn bulk_build_materializes_activity_bundles_from_single_pass() {
    let block = same_block_create_then_consume_fixture();
    let create_tx_hash = hex::decode(&block.block.transactions[0].hash[2..]).expect("create tx hash");
    let consume_tx_hash =
        hex::decode(&block.block.transactions[1].hash[2..]).expect("consume tx hash");
    let lock_hash = ScriptParser::compute_script_hash(&fixture_lock_script());

    let snapshot =
        materialize_bulk_artifacts_for_test(&[block]).expect("bulk build artifact snapshot");

    assert_eq!(snapshot.activity_bundles.len(), 2);

    let create_key = keys::encode_tx_activity_bundle_key(14_000_321, 0, &create_tx_hash);
    let consume_key = keys::encode_tx_activity_bundle_key(14_000_321, 1, &consume_tx_hash);

    let create_bundle = snapshot
        .activity_bundles
        .get(&create_key)
        .expect("cellbase activity bundle");
    assert!(create_bundle.is_cellbase);
    assert_eq!(create_bundle.owners.len(), 1);
    assert_eq!(create_bundle.owners[0].lock_hash, lock_hash);

    let consume_bundle = snapshot
        .activity_bundles
        .get(&consume_key)
        .expect("consume activity bundle");
    assert!(!consume_bundle.is_cellbase);
    assert_eq!(consume_bundle.owners.len(), 1);
    assert_eq!(consume_bundle.owners[0].lock_hash, lock_hash);
    assert_eq!(consume_bundle.owners[0].ckb_delta, 0);
    assert!(consume_bundle.owners[0].asset_changes.is_empty());
}
