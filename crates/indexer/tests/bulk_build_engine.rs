use ckbadger_indexer::parser::spore::{CLUSTER_CODE_HASH_MAINNET_V2, SPORE_CODE_HASH_MAINNET_V2};
use ckbadger_indexer::parser::ScriptParser;
use ckbadger_indexer::rpc::{
    BlockResponseWithCycles, BlockView, CellInput, CellOutput, HeaderView, OutPoint, Script,
    TransactionView,
};
use ckbadger_indexer::sync::{
    materialize_core_owner_state_for_test, resolve_live_cell_snapshot_for_test, CellSemanticTag,
    CoreOwnerStateSnapshot,
};

fn fixture_lock_script() -> Script {
    Script {
        code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8".to_string(),
        hash_type: "type".to_string(),
        args: "0x927f3e74dceb87c81ba65a19da4f098b4de75a0d".to_string(),
    }
}

fn fixture_header(number: u64) -> HeaderView {
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
            header: fixture_header(14_000_321),
            uncles: vec![],
            transactions: vec![create_tx, consume_tx],
            proposals: vec![],
        },
        cycles: None,
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

fn cluster_and_spore_fixture() -> Vec<BlockResponseWithCycles> {
    let cluster_id = [0x44; 32];
    let spore_id = [0x55; 32];
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
        outputs: vec![
            CellOutput {
                capacity: format!("0x{:x}", 400_00000000u64),
                lock: fixture_lock_script(),
                type_: Some(create_cluster_type_script(&cluster_id)),
            },
            CellOutput {
                capacity: format!("0x{:x}", 400_00000000u64),
                lock: fixture_lock_script(),
                type_: Some(create_spore_type_script(&spore_id)),
            },
        ],
        outputs_data: vec![
            format!(
                "0x{}",
                hex::encode(create_cluster_data("Engine Cluster", "shared owner pass"))
            ),
            format!(
                "0x{}",
                hex::encode(create_spore_data(
                    "image/png",
                    b"bulk-build",
                    Some(&cluster_id)
                ))
            ),
        ],
        witnesses: vec!["0x".to_string()],
    };

    vec![BlockResponseWithCycles {
        block: BlockView {
            header: fixture_header(14_000_654),
            uncles: vec![],
            transactions: vec![create_tx],
            proposals: vec![],
        },
        cycles: None,
    }]
}

#[test]
fn bulk_build_live_resolution_handles_same_block_create_then_consume() {
    let snapshot = resolve_live_cell_snapshot_for_test(&[same_block_create_then_consume_fixture()])
        .expect("same-block live-cell resolution");

    assert_eq!(snapshot.txs.len(), 2);
    assert_eq!(snapshot.txs[0].tx_index, 0);
    assert_eq!(snapshot.txs[1].tx_index, 1);
    assert!(snapshot.txs[0].resolved_inputs.is_empty());
    assert_eq!(snapshot.txs[1].resolved_inputs.len(), 1);
    assert_eq!(snapshot.txs[1].resolved_inputs[0].capacity, 100_00000000);
    assert_eq!(
        snapshot.txs[1].resolved_inputs[0].occupied_capacity,
        61_00000000
    );
    assert_eq!(
        snapshot.txs[1].resolved_inputs[0].semantic_tag,
        CellSemanticTag::Plain
    );
    assert_eq!(snapshot.remaining_live_cells, 1);
}

#[test]
fn bulk_build_core_owner_pass_materializes_multiple_reducers_from_single_resolved_run() {
    let cluster_id = [0x44; 32];
    let spore_id = [0x55; 32];
    let lock_hash = ScriptParser::compute_script_hash(&fixture_lock_script());
    let spore_code_hash = hex::decode(&SPORE_CODE_HASH_MAINNET_V2[2..]).expect("spore code hash");

    let snapshot: CoreOwnerStateSnapshot =
        materialize_core_owner_state_for_test(&cluster_and_spore_fixture())
            .expect("core owner snapshot");

    assert!(snapshot.address_balances.contains_key(lock_hash.as_slice()));
    assert!(snapshot
        .script_infos
        .contains_key(spore_code_hash.as_slice()));
    assert!(snapshot
        .object_state
        .spores
        .contains_key(cluster_id.as_slice()));
    assert!(snapshot
        .object_state
        .spores
        .contains_key(spore_id.as_slice()));
    assert!(snapshot.token_state.tokens.is_empty());
    assert!(snapshot.dao_state.deposits.is_empty());
}
