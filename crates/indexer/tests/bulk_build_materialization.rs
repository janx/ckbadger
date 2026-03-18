use ckbadger_indexer::rpc::{
    BlockResponseWithCycles, BlockView, CellInput, CellOutput, HeaderView, OutPoint, Script,
    TransactionView,
};
use ckbadger_indexer::parser::ScriptParser;
use ckbadger_indexer::sync::{
    materialize_bulk_artifacts_for_test, run_sample_bulk_materialization_for_test,
};

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

    assert_eq!(snapshot.report.streamed_history_rows, 6);
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
