use ckbadger_indexer::parser::spore::{
    CLUSTER_CODE_HASH_MAINNET_V2, SPORE_CODE_HASH_MAINNET_DID, SPORE_CODE_HASH_MAINNET_V2,
};
use ckbadger_indexer::parser::ScriptParser;
use ckbadger_indexer::rpc::{
    BlockResponseWithCycles, BlockView, CellInput, CellOutput, HeaderView, OutPoint, Script,
    TransactionView,
};
use ckbadger_indexer::sync::{
    materialize_bulk_artifacts_for_test, materialize_bulk_artifacts_from_batches_for_test,
    run_sample_bulk_materialization_for_test,
};
use ckbadger_store::keys;
use ckbadger_store::SyncStatus;
use ckbadger_store::types::ConsumedCellMeta;
use ckbadger_store::types::DID_CKB_SENTINEL_COLLECTION;

fn fixture_lock_script() -> Script {
    Script {
        code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8".to_string(),
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

fn encode_molecule_bytes(data: &[u8]) -> Vec<u8> {
    let len = data.len() as u32;
    let mut result = len.to_le_bytes().to_vec();
    result.extend_from_slice(data);
    result
}

fn create_spore_type_script(spore_id: &[u8; 32]) -> Script {
    Script {
        code_hash: SPORE_CODE_HASH_MAINNET_V2.to_string(),
        hash_type: "data1".to_string(),
        args: format!("0x{}", hex::encode(spore_id)),
    }
}

fn create_did_type_script(did_id: &[u8; 32]) -> Script {
    Script {
        code_hash: SPORE_CODE_HASH_MAINNET_DID.to_string(),
        hash_type: "type".to_string(),
        args: format!("0x{}", hex::encode(did_id)),
    }
}

fn create_cluster_type_script(cluster_id: &[u8; 32]) -> Script {
    Script {
        code_hash: CLUSTER_CODE_HASH_MAINNET_V2.to_string(),
        hash_type: "data1".to_string(),
        args: format!("0x{}", hex::encode(cluster_id)),
    }
}

fn create_spore_data(content_type: &str, content: &[u8], cluster_id: Option<&[u8; 32]>) -> Vec<u8> {
    let content_type_bytes = encode_molecule_bytes(content_type.as_bytes());
    let content_bytes = encode_molecule_bytes(content);
    let cluster_id_bytes = cluster_id.map(|id| encode_molecule_bytes(id));

    let offset_content_type = 16u32;
    let offset_content = offset_content_type + content_type_bytes.len() as u32;
    let offset_cluster_id = offset_content + content_bytes.len() as u32;
    let total_size = offset_cluster_id
        + cluster_id_bytes
            .as_ref()
            .map(|bytes| bytes.len())
            .unwrap_or(0) as u32;

    let mut data = Vec::new();
    data.extend_from_slice(&total_size.to_le_bytes());
    data.extend_from_slice(&offset_content_type.to_le_bytes());
    data.extend_from_slice(&offset_content.to_le_bytes());
    data.extend_from_slice(&offset_cluster_id.to_le_bytes());
    data.extend_from_slice(&content_type_bytes);
    data.extend_from_slice(&content_bytes);
    if let Some(cluster_id_bytes) = cluster_id_bytes {
        data.extend_from_slice(&cluster_id_bytes);
    }
    data
}

fn create_cluster_data(name: &str, description: &str) -> Vec<u8> {
    let name_bytes = encode_molecule_bytes(name.as_bytes());
    let description_bytes = encode_molecule_bytes(description.as_bytes());
    let offset_name = 16u32;
    let offset_description = offset_name + name_bytes.len() as u32;
    let offset_end = offset_description + description_bytes.len() as u32;

    let mut data = Vec::new();
    data.extend_from_slice(&offset_end.to_le_bytes());
    data.extend_from_slice(&offset_name.to_le_bytes());
    data.extend_from_slice(&offset_description.to_le_bytes());
    data.extend_from_slice(&offset_end.to_le_bytes());
    data.extend_from_slice(&name_bytes);
    data.extend_from_slice(&description_bytes);
    data
}

fn object_activity_fixture() -> Vec<BlockResponseWithCycles> {
    let cluster_id = [0x11; 32];
    let spore_id = [0x22; 32];
    let did_id = [0x33; 32];

    let create_tx = TransactionView {
        hash: format!("0x{}", "a1".repeat(32)),
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
        outputs: vec![
            CellOutput {
                capacity: format!("0x{:x}", 220_00000000u64),
                lock: fixture_lock_script(),
                type_: Some(create_cluster_type_script(&cluster_id)),
            },
            CellOutput {
                capacity: format!("0x{:x}", 220_00000000u64),
                lock: fixture_lock_script(),
                type_: Some(create_spore_type_script(&spore_id)),
            },
            CellOutput {
                capacity: format!("0x{:x}", 150_00000000u64),
                lock: Script {
                    code_hash: fixture_lock_script().code_hash,
                    hash_type: fixture_lock_script().hash_type,
                    args: format!("0x{}", "03".repeat(20)),
                },
                type_: Some(create_did_type_script(&did_id)),
            },
        ],
        outputs_data: vec![
            format!(
                "0x{}",
                hex::encode(create_cluster_data(
                    "Genesis Cluster",
                    "{\"dob\":{\"ver\":1}}"
                ))
            ),
            format!(
                "0x{}",
                hex::encode(create_spore_data(
                    "image/png",
                    b"spore-content",
                    Some(&cluster_id)
                ))
            ),
            "0x".to_string(),
        ],
        witnesses: vec!["0x".to_string()],
    };

    let dummy_cellbase = TransactionView {
        hash: format!("0x{}", "b0".repeat(32)),
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
            capacity: format!("0x{:x}", 500_00000000u64),
            lock: Script {
                code_hash: fixture_lock_script().code_hash,
                hash_type: fixture_lock_script().hash_type,
                args: format!("0x{}", "09".repeat(20)),
            },
            type_: None,
        }],
        outputs_data: vec!["0x".to_string()],
        witnesses: vec!["0x".to_string()],
    };

    let transfer_and_burn_tx = TransactionView {
        hash: format!("0x{}", "b1".repeat(32)),
        version: "0x0".to_string(),
        cell_deps: vec![],
        header_deps: vec![],
        inputs: vec![
            CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: create_tx.hash.clone(),
                    index: "0x1".to_string(),
                },
            },
            CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: create_tx.hash.clone(),
                    index: "0x2".to_string(),
                },
            },
        ],
        outputs: vec![CellOutput {
            capacity: format!("0x{:x}", 220_00000000u64),
            lock: Script {
                code_hash: fixture_lock_script().code_hash,
                hash_type: fixture_lock_script().hash_type,
                args: format!("0x{}", "02".repeat(20)),
            },
            type_: Some(create_spore_type_script(&spore_id)),
        }],
        outputs_data: vec![format!(
            "0x{}",
            hex::encode(create_spore_data(
                "image/png",
                b"spore-content",
                Some(&cluster_id)
            ))
        )],
        witnesses: vec!["0x".to_string()],
    };

    vec![
        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header(14_001_000, 0x81),
                uncles: vec![],
                transactions: vec![create_tx],
                proposals: vec![],
            },
            cycles: None,
        },
        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header(14_001_001, 0x82),
                uncles: vec![],
                transactions: vec![dummy_cellbase, transfer_and_burn_tx],
                proposals: vec![],
            },
            cycles: None,
        },
    ]
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
    let create_tx_hash =
        hex::decode(&block.block.transactions[0].hash[2..]).expect("create tx hash");
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

    let create_tx = snapshot
        .txs_by_hash
        .get(&create_tx_hash)
        .expect("create tx");
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

    assert!(snapshot
        .core
        .address_balances
        .contains_key(lock_hash.as_slice()));
}

#[test]
fn bulk_build_materializes_append_only_cells_and_final_live_markers_from_single_pass() {
    let block = same_block_create_then_consume_fixture();
    let create_tx_hash =
        hex::decode(&block.block.transactions[0].hash[2..]).expect("create tx hash");
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
    assert_eq!(
        snapshot.cell_payloads[&create_outpoint].capacity,
        100_00000000
    );
    assert_eq!(
        snapshot.cell_payloads[&consume_outpoint].capacity,
        100_00000000
    );

    assert_eq!(snapshot.live_cells.len(), 1);
    assert!(!snapshot.live_cells.contains_key(&create_outpoint));
    assert_eq!(
        snapshot.live_cells.get(&consume_outpoint),
        Some(&14_000_321)
    );
}

#[test]
fn bulk_build_materializes_consumed_cells_and_live_cell_indexes_from_single_pass() {
    let block = same_block_typed_data_create_then_consume_fixture();
    let create_tx_hash =
        hex::decode(&block.block.transactions[0].hash[2..]).expect("create tx hash");
    let consume_tx_hash =
        hex::decode(&block.block.transactions[1].hash[2..]).expect("consume tx hash");
    let create_outpoint = keys::encode_outpoint(&create_tx_hash, 0).to_vec();
    let lock_hash = ScriptParser::compute_script_hash(&fixture_lock_script());
    let lock_code_hash =
        hex::decode(&fixture_lock_script().code_hash[2..]).expect("lock code hash");
    let type_hash = ScriptParser::compute_script_hash(&fixture_type_script());
    let type_code_hash =
        hex::decode(&fixture_type_script().code_hash[2..]).expect("type code hash");
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

    let expected_live_lock =
        keys::encode_cell_index_key(&lock_hash, 14_000_777, &consume_tx_hash, 0);
    let expected_live_lock_code =
        keys::encode_cell_index_key(&lock_code_hash, 14_000_777, &consume_tx_hash, 0);
    let expected_live_type =
        keys::encode_cell_index_key(&type_hash, 14_000_777, &consume_tx_hash, 0);
    let expected_live_type_code =
        keys::encode_cell_index_key(&type_code_hash, 14_000_777, &consume_tx_hash, 0);
    let old_lock_key = keys::encode_cell_index_key(&lock_hash, 14_000_777, &create_tx_hash, 0);

    assert_eq!(snapshot.cell_by_lock.len(), 1);
    assert!(snapshot.cell_by_lock.contains(&expected_live_lock));
    assert!(!snapshot.cell_by_lock.contains(&old_lock_key));
    assert_eq!(snapshot.cell_by_lock_code.len(), 1);
    assert!(snapshot
        .cell_by_lock_code
        .contains(&expected_live_lock_code));
    assert_eq!(snapshot.cell_by_type.len(), 1);
    assert!(snapshot.cell_by_type.contains(&expected_live_type));
    assert_eq!(snapshot.cell_by_type_code.len(), 1);
    assert!(snapshot
        .cell_by_type_code
        .contains(&expected_live_type_code));

    let expected_data_hash_create =
        keys::encode_cell_index_key(&data_hash, 14_000_777, &create_tx_hash, 0);
    let expected_data_hash_consume =
        keys::encode_cell_index_key(&data_hash, 14_000_777, &consume_tx_hash, 0);
    assert_eq!(snapshot.cell_by_data_hash.len(), 2);
    assert!(snapshot
        .cell_by_data_hash
        .contains(&expected_data_hash_create));
    assert!(snapshot
        .cell_by_data_hash
        .contains(&expected_data_hash_consume));
}

#[test]
fn bulk_build_materializes_activity_bundles_from_single_pass() {
    let block = same_block_create_then_consume_fixture();
    let create_tx_hash =
        hex::decode(&block.block.transactions[0].hash[2..]).expect("create tx hash");
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

#[test]
fn bulk_build_materializes_sealed_activity_stats_from_single_pass() {
    let block = same_block_create_then_consume_fixture();

    let snapshot =
        materialize_bulk_artifacts_for_test(&[block]).expect("bulk build artifact snapshot");

    assert!(
        snapshot.report.sealed_aggregate_rows > 0,
        "bulk build should materialize sealed activity stats rows"
    );
    assert_eq!(snapshot.daily_activity_stats.len(), 1);
    assert_eq!(snapshot.hourly_activity_stats.len(), 1);

    let daily = snapshot
        .daily_activity_stats
        .values()
        .next()
        .expect("daily activity stats");
    assert_eq!(daily.coinbase_count, 1);
    assert_eq!(daily.transfer_count, 1);
    assert_eq!(daily.unique_address_count, 1);
    assert_eq!(daily.total_ckb_moved, 0);

    let hourly = snapshot
        .hourly_activity_stats
        .values()
        .next()
        .expect("hourly activity stats");
    assert_eq!(hourly.coinbase_count, 1);
    assert_eq!(hourly.transfer_count, 1);
    assert_eq!(hourly.unique_address_count, 1);
    assert_eq!(hourly.total_ckb_moved, 0);
}

#[test]
fn bulk_build_materializes_did_activity_count_from_collection_activity_history() {
    let snapshot = materialize_bulk_artifacts_for_test(&object_activity_fixture())
        .expect("bulk build artifact snapshot");

    let did_agg = snapshot
        .core
        .object_state
        .did_agg
        .as_ref()
        .expect("did collection aggregate");
    assert_eq!(did_agg.activities_count, 1);
    assert_eq!(did_agg.total_count, 1);
    assert_eq!(did_agg.live_count, 0);
    assert_eq!(did_agg.holders_count, 0);
    assert_eq!(
        snapshot
            .core
            .object_state
            .identities_by_collection
            .get(DID_CKB_SENTINEL_COLLECTION.as_slice())
            .expect("did collection identities")
            .len(),
        1
    );
}

#[test]
fn bulk_build_multi_batch_materialization_matches_single_pass_for_cross_batch_state() {
    let blocks = object_activity_fixture();
    let split_batches = vec![vec![blocks[0].clone()], vec![blocks[1].clone()]];

    let single =
        materialize_bulk_artifacts_for_test(&blocks).expect("single-pass bulk build artifact");
    let split = materialize_bulk_artifacts_from_batches_for_test(&split_batches)
        .expect("multi-batch bulk build artifact");

    assert_eq!(
        split.report.streamed_history_rows,
        single.report.streamed_history_rows
    );
    assert_eq!(
        split.report.sealed_aggregate_rows,
        single.report.sealed_aggregate_rows
    );
    assert_eq!(
        split.report.final_snapshot_rows,
        single.report.final_snapshot_rows
    );
    assert_eq!(split.activity_bundles.len(), single.activity_bundles.len());
    assert_eq!(split.live_cells.len(), single.live_cells.len());
    assert_eq!(split.consumed_cells.len(), single.consumed_cells.len());
    assert_eq!(
        split.core.object_state.spores.len(),
        single.core.object_state.spores.len()
    );
    assert_eq!(
        split
            .core
            .object_state
            .did_agg
            .as_ref()
            .expect("split did agg")
            .activities_count,
        single
            .core
            .object_state
            .did_agg
            .as_ref()
            .expect("single did agg")
            .activities_count
    );

    let split_daily = split
        .daily_activity_stats
        .values()
        .next()
        .expect("split daily activity stats");
    let single_daily = single
        .daily_activity_stats
        .values()
        .next()
        .expect("single daily activity stats");
    assert_eq!(split_daily.coinbase_count, single_daily.coinbase_count);
    assert_eq!(split_daily.transfer_count, single_daily.transfer_count);
    assert_eq!(
        split_daily.unique_address_count,
        single_daily.unique_address_count
    );

    let split_hourly = split
        .hourly_activity_stats
        .values()
        .next()
        .expect("split hourly activity stats");
    let single_hourly = single
        .hourly_activity_stats
        .values()
        .next()
        .expect("single hourly activity stats");
    assert_eq!(split_hourly.coinbase_count, single_hourly.coinbase_count);
    assert_eq!(split_hourly.transfer_count, single_hourly.transfer_count);
    assert_eq!(
        split_hourly.unique_address_count,
        single_hourly.unique_address_count
    );
}

#[test]
fn bulk_build_session_materialization_sets_final_sync_status_and_clears_marker() {
    let blocks = object_activity_fixture();
    let split_batches = vec![vec![blocks[0].clone()], vec![blocks[1].clone()]];

    let snapshot = materialize_bulk_artifacts_from_batches_for_test(&split_batches)
        .expect("multi-batch bulk build artifact");
    let status: SyncStatus = snapshot.sync_status.clone();

    assert_eq!(status.tip_block_number, 14_001_001);
    assert_eq!(status.total_transactions, 3);
    assert_eq!(status.total_cells_created, 5);
    assert_eq!(status.total_cells_consumed, 2);
    assert!(status.sync_started_at.is_some());
    assert!(status.bulk_sync_completed_at.is_some());
    assert_eq!(status.bulk_sync_completed_block, Some(14_001_001));
    assert!(
        snapshot.bulk_build_session_marker.is_none(),
        "successful bulk-build session must clear the in-progress marker"
    );
}
