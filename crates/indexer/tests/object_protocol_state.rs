use ckbadger_indexer::parser::spore::{
    CLUSTER_CODE_HASH_MAINNET_V2, SPORE_CODE_HASH_MAINNET_DID, SPORE_CODE_HASH_MAINNET_V2,
};
use ckbadger_indexer::parser::ScriptParser;
use ckbadger_indexer::rpc::{
    BlockResponseWithCycles, BlockView, CellInput, CellOutput, HeaderView, OutPoint, Script,
    TransactionView,
};
use ckbadger_indexer::sync::{materialize_object_state_for_test, ObjectStateSnapshot};
use ckbadger_store::types::{DID_CKB_SENTINEL_COLLECTION, ObjectStandard};

fn fixture_lock_script(args_hex: &str) -> Script {
    Script {
        code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8".to_string(),
        hash_type: "type".to_string(),
        args: args_hex.to_string(),
    }
}

fn fixture_header(number: u64, hash_byte: u8, timestamp_ms: u64) -> HeaderView {
    HeaderView {
        version: "0x0".to_string(),
        compact_target: "0x1a08a97e".to_string(),
        timestamp: format!("0x{timestamp_ms:x}"),
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

fn create_spore_data(
    content_type: &str,
    content: &[u8],
    cluster_id: Option<&[u8; 32]>,
) -> Vec<u8> {
    let content_type_bytes = encode_molecule_bytes(content_type.as_bytes());
    let content_bytes = encode_molecule_bytes(content);
    let cluster_id_bytes = cluster_id.map(|id| encode_molecule_bytes(id));

    let offset_content_type = 16u32;
    let offset_content = offset_content_type + content_type_bytes.len() as u32;
    let offset_cluster_id = offset_content + content_bytes.len() as u32;
    let total_size =
        offset_cluster_id + cluster_id_bytes.as_ref().map(|bytes| bytes.len()).unwrap_or(0) as u32;

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

fn bulk_build_object_fixture() -> Vec<BlockResponseWithCycles> {
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
                capacity: format!("0x{:x}", 200_00000000u64),
                lock: fixture_lock_script(&format!("0x{}", "01".repeat(20))),
                type_: Some(create_cluster_type_script(&cluster_id)),
            },
            CellOutput {
                capacity: format!("0x{:x}", 200_00000000u64),
                lock: fixture_lock_script(&format!("0x{}", "01".repeat(20))),
                type_: Some(create_spore_type_script(&spore_id)),
            },
            CellOutput {
                capacity: format!("0x{:x}", 150_00000000u64),
                lock: fixture_lock_script(&format!("0x{}", "03".repeat(20))),
                type_: Some(create_did_type_script(&did_id)),
            },
        ],
        outputs_data: vec![
            format!("0x{}", hex::encode(create_cluster_data("Genesis Cluster", "{\"dob\":{\"ver\":1}}"))),
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
            lock: fixture_lock_script(&format!("0x{}", "09".repeat(20))),
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
            capacity: format!("0x{:x}", 200_00000000u64),
            lock: fixture_lock_script(&format!("0x{}", "02".repeat(20))),
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
                header: fixture_header(14_001_000, 0x81, 1_700_000_000_000),
                uncles: vec![],
                transactions: vec![create_tx.clone()],
                proposals: vec![],
            },
            cycles: None,
        },
        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header(14_001_001, 0x82, 1_700_000_010_000),
                uncles: vec![],
                transactions: vec![dummy_cellbase, transfer_and_burn_tx],
                proposals: vec![],
            },
            cycles: None,
        },
    ]
}

#[test]
fn bulk_build_object_owner_materializes_spore_cluster_and_did_state_without_db_reads() {
    let cluster_id = [0x11; 32];
    let spore_id = [0x22; 32];
    let did_id = [0x33; 32];
    let lock_a_hash =
        ScriptParser::compute_script_hash(&fixture_lock_script(&format!("0x{}", "01".repeat(20))));
    let lock_b_hash =
        ScriptParser::compute_script_hash(&fixture_lock_script(&format!("0x{}", "02".repeat(20))));
    let create_tx_hash = vec![0xa1; 32];
    let transfer_tx_hash = vec![0xb1; 32];
    let spore_type_hash =
        ScriptParser::compute_script_hash(&create_spore_type_script(&spore_id));

    let snapshot: ObjectStateSnapshot =
        materialize_object_state_for_test(&bulk_build_object_fixture()).expect("object snapshot");

    let stored_spore = snapshot
        .spores
        .get(spore_id.as_slice())
        .expect("spore entry exists");
    assert_eq!(stored_spore.standard, ObjectStandard::Spore);
    assert!(stored_spore.is_live);
    assert_eq!(stored_spore.owner_lock_hash, Some(lock_b_hash.clone()));
    assert_eq!(stored_spore.collection_id, Some(cluster_id.to_vec()));
    assert_eq!(stored_spore.created_at_tx, create_tx_hash);

    let stored_cluster = snapshot
        .spores
        .get(cluster_id.as_slice())
        .expect("cluster entry exists");
    assert_eq!(stored_cluster.standard, ObjectStandard::SporeCluster);
    assert_eq!(
        stored_cluster.description.as_deref(),
        Some("{\"dob\":{\"ver\":1}}")
    );

    let cluster_agg = snapshot
        .cluster_aggs
        .get(cluster_id.as_slice())
        .expect("cluster aggregate exists");
    assert_eq!(cluster_agg.total_count, 1);
    assert_eq!(cluster_agg.live_count, 1);
    assert_eq!(cluster_agg.owner_count, 1);

    let members = snapshot
        .spores_by_cluster
        .get(cluster_id.as_slice())
        .expect("cluster membership exists");
    assert_eq!(members, &vec![spore_id.to_vec()]);

    let cluster_owner_counts = snapshot
        .cluster_owner_counts
        .get(cluster_id.as_slice())
        .expect("cluster owner counts exist");
    assert_eq!(cluster_owner_counts.get(lock_a_hash.as_slice()), None);
    assert_eq!(cluster_owner_counts.get(lock_b_hash.as_slice()), Some(&1));

    let did_entry = snapshot
        .identities
        .get(did_id.as_slice())
        .expect("did entry exists");
    assert!(!did_entry.is_live);
    assert!(did_entry.owner_lock_hash.is_none());

    let did_agg = snapshot.did_agg.expect("did aggregate exists");
    assert_eq!(did_agg.total_count, 1);
    assert_eq!(did_agg.live_count, 0);
    assert_eq!(did_agg.holders_count, 0);

    assert_eq!(
        snapshot
            .identities_by_collection
            .get(DID_CKB_SENTINEL_COLLECTION.as_slice())
            .expect("did collection ids"),
        &vec![did_id.to_vec()]
    );
    assert!(snapshot.did_owner_counts.is_empty());

    let outpoints = snapshot
        .spore_outpoints
        .get(spore_id.as_slice())
        .expect("spore outpoints exist");
    assert_eq!(
        outpoints,
        &vec![(create_tx_hash, 1_i16), (transfer_tx_hash, 0_i16)]
    );

    let type_index = snapshot
        .spore_type_indexes
        .get(spore_type_hash.as_slice())
        .expect("spore type index exists");
    assert_eq!(type_index.spore_id, spore_id.to_vec());
    assert_eq!(type_index.cluster_id, Some(cluster_id.to_vec()));
}
