use ckbadger_common::dao::SHANNON;
use ckbadger_indexer::parser::bit_cell::BIT_CELL_CODE_HASH_TESTNET;
use ckbadger_indexer::parser::spore::{CLUSTER_CODE_HASH_MAINNET_V2, SPORE_CODE_HASH_MAINNET_V2};
use ckbadger_indexer::parser::ScriptParser;
use ckbadger_indexer::rpc::{
    BlockResponseWithCycles, BlockView, CellInput, CellOutput, DaoField, HeaderView, OutPoint,
    Script, TransactionView,
};
use ckbadger_indexer::sync::{
    materialize_bulk_artifacts_for_test, materialize_bulk_artifacts_from_batches_for_test,
    materialize_bulk_stage_for_test, materialize_bulk_stage_then_complete_sync_status_for_test,
    run_sample_bulk_materialization_for_test, simulate_startup_sync_path_for_test,
};
use ckbadger_store::keys;
use ckbadger_store::types::{ConsumedCellMeta, BIT_CELL_SENTINEL_COLLECTION};
use ckbadger_store::SyncStatus;

/// Real mainnet cellbase first witness (block 12,000,000): block parsing
/// requires every non-genesis cellbase to carry a valid RFC-0022
/// `CellbaseWitness`.
const TEST_CELLBASE_WITNESS: &str = "0x7a0000000c00000055000000490000001000000030000000310000009bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce801140000008211f1b938a107cd53b6302cc752a6fc3965638d210000000000000020302e3131332e3020283832383731613320323032342d30312d303929";

fn fixture_lock_script() -> Script {
    Script {
        code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8".to_string(),
        hash_type: "type".to_string(),
        args: "0x927f3e74dceb87c81ba65a19da4f098b4de75a0d".to_string(),
    }
}

fn fixture_header(number: u64, hash_byte: u8) -> HeaderView {
    fixture_header_with_timestamp(number, hash_byte, 1_710_000_000_000)
}

/// C (total_issuance) carried by every fixture header. Must exceed `U` for the
/// RFC-0023 secondary-issuance split, and be at least genesis issuance for the
/// latest DAO statistics / APC validation.
const FIXTURE_DAO_C: u64 = 33_600_000_000 * SHANNON;

/// U (occupied_capacity) carried by every fixture header. Must be < C.
const FIXTURE_DAO_U: u64 = 100_000_000_000_000;

fn fixture_header_with_timestamp(number: u64, hash_byte: u8, timestamp_ms: i64) -> HeaderView {
    // A real chain header always carries a valid DAO field; an all-zero one
    // would make C == U == 0, which is not a chain state the protocol can
    // split secondary issuance against.
    let mut dao = [0u8; 32];
    dao[0..8].copy_from_slice(&FIXTURE_DAO_C.to_le_bytes());
    dao[24..32].copy_from_slice(&FIXTURE_DAO_U.to_le_bytes());
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
        dao: format!("0x{}", hex::encode(dao)),
        nonce: "0x1".to_string(),
        hash: format!("0x{}", format!("{hash_byte:02x}").repeat(32)),
    }
}

fn fixture_lock_script_b() -> Script {
    Script {
        code_hash: fixture_lock_script().code_hash,
        hash_type: fixture_lock_script().hash_type,
        args: format!("0x{}", "02".repeat(20)),
    }
}

fn fixture_lock_script_with_args(args_hex: &str) -> Script {
    Script {
        code_hash: fixture_lock_script().code_hash,
        hash_type: fixture_lock_script().hash_type,
        args: args_hex.to_string(),
    }
}

fn fixture_header_with_ar(number: u64, hash_byte: u8, ar: u64, timestamp_ms: i64) -> HeaderView {
    let mut header = fixture_header_with_timestamp(number, hash_byte, timestamp_ms);
    let mut dao = [0u8; 32];
    dao[0..8].copy_from_slice(&FIXTURE_DAO_C.to_le_bytes());
    // AR (accumulated rate)
    dao[8..16].copy_from_slice(&ar.to_le_bytes());
    dao[24..32].copy_from_slice(&FIXTURE_DAO_U.to_le_bytes());
    header.dao = format!("0x{}", hex::encode(dao));
    header
}

fn fixture_dao_type_script() -> Script {
    Script {
        code_hash: "0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e".to_string(),
        hash_type: "type".to_string(),
        args: "0x".to_string(),
    }
}

#[test]
fn fixture_header_with_ar_uses_total_issuance_above_genesis_burn() {
    // 8.4B CKB burnt at genesis (in shannons); the unspendable Satoshi gift.
    // Formerly `ckbadger_common::dao::GENESIS_BURNT`, now derived per-network
    // into `GenesisBaseline::burnt`. Inlined here as the mainnet network
    // invariant this fixture models.
    const GENESIS_BURNT_SHANNONS: u128 = 840_000_000_000_000_000;
    let header = fixture_header_with_ar(100, 0xaa, 10_000_000_000_000_000, 1_710_000_000_000);
    let dao = DaoField::from_hex(&header.dao).expect("parse dao field");

    assert!(
        u128::from(dao.total_issuance) >= GENESIS_BURNT_SHANNONS,
        "fixture dao total issuance {} must be >= genesis burnt {}",
        dao.total_issuance,
        GENESIS_BURNT_SHANNONS
    );
}

fn bulk_build_dao_activity_fixture() -> Vec<BlockResponseWithCycles> {
    let dao_type = fixture_dao_type_script();
    let deposit_lock = fixture_lock_script_with_args(&format!("0x{}", "11".repeat(20)));

    let deposit_tx = TransactionView {
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
        outputs: vec![CellOutput {
            capacity: format!("0x{:x}", 200_00000000u64),
            lock: deposit_lock.clone(),
            type_: Some(dao_type.clone()),
        }],
        outputs_data: vec![format!("0x{}", "00".repeat(8))],
        witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
    };

    let request_tx = TransactionView {
        hash: format!("0x{}", "a2".repeat(32)),
        version: "0x0".to_string(),
        cell_deps: vec![],
        header_deps: vec![],
        inputs: vec![CellInput {
            since: "0x0".to_string(),
            previous_output: OutPoint {
                tx_hash: deposit_tx.hash.clone(),
                index: "0x0".to_string(),
            },
        }],
        outputs: vec![CellOutput {
            capacity: format!("0x{:x}", 200_00000000u64),
            lock: deposit_lock.clone(),
            type_: Some(dao_type),
        }],
        outputs_data: vec![format!("0x{}", hex::encode(100u64.to_le_bytes()))],
        witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
    };

    let completion_tx = TransactionView {
        hash: format!("0x{}", "a3".repeat(32)),
        version: "0x0".to_string(),
        cell_deps: vec![],
        header_deps: vec![],
        inputs: vec![CellInput {
            since: "0x0".to_string(),
            previous_output: OutPoint {
                tx_hash: request_tx.hash.clone(),
                index: "0x0".to_string(),
            },
        }],
        outputs: vec![CellOutput {
            capacity: format!("0x{:x}", 219_60000000u64),
            lock: deposit_lock,
            type_: None,
        }],
        outputs_data: vec!["0x".to_string()],
        witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
    };

    vec![
        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header_with_ar(100, 0xa1, 10_000, 1_700_300_000_000),
                uncles: vec![],
                transactions: vec![deposit_tx],
                proposals: vec![],
            },
            cycles: None,
        },
        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header_with_ar(101, 0xa2, 12_000, 1_700_300_010_000),
                uncles: vec![],
                transactions: vec![request_tx],
                proposals: vec![],
            },
            cycles: None,
        },
        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header_with_ar(102, 0xa3, 13_000, 1_700_300_020_000),
                uncles: vec![],
                transactions: vec![completion_tx],
                proposals: vec![],
            },
            cycles: None,
        },
    ]
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
        witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
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

fn create_bit_cell_type_script() -> Script {
    Script {
        code_hash: BIT_CELL_CODE_HASH_TESTNET.to_string(),
        hash_type: "type".to_string(),
        args: "0x".to_string(),
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
                capacity: format!("0x{:x}", 200_00000000u64),
                lock: Script {
                    code_hash: fixture_lock_script().code_hash,
                    hash_type: fixture_lock_script().hash_type,
                    args: format!("0x{}", "03".repeat(20)),
                },
                type_: Some(create_bit_cell_type_script()),
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
            "0x000000003c00000010000000240000002c000000a7d4860aaf1dc83daedf75d6022811d2c2ae250b1b46fc69000000000c00000032303234303530372e626974".to_string(),
        ],
        witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
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
        witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
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
        witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
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

fn hodl_tracker_fixture() -> Vec<BlockResponseWithCycles> {
    let create_tx = TransactionView {
        hash: format!("0x{}", "e1".repeat(32)),
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
            capacity: "0x342770c00".to_string(),
            lock: fixture_lock_script(),
            type_: None,
        }],
        outputs_data: vec!["0x".to_string()],
        witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
    };

    let transfer_tx = TransactionView {
        hash: format!("0x{}", "e2".repeat(32)),
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
        outputs: vec![
            CellOutput {
                capacity: "0x1dcd65000".to_string(),
                lock: fixture_lock_script(),
                type_: None,
            },
            CellOutput {
                capacity: "0x165a0bc00".to_string(),
                lock: fixture_lock_script_b(),
                type_: None,
            },
        ],
        outputs_data: vec!["0x".to_string(), "0x".to_string()],
        witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
    };

    vec![
        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header_with_timestamp(14_002_000, 0x91, 1_705_276_800_000),
                uncles: vec![],
                transactions: vec![create_tx],
                proposals: vec![],
            },
            cycles: None,
        },
        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header_with_timestamp(14_002_001, 0x92, 1_705_280_400_000),
                uncles: vec![],
                transactions: vec![transfer_tx],
                proposals: vec![],
            },
            cycles: None,
        },
    ]
}

fn cell_distribution_snapshot_fixture() -> Vec<BlockResponseWithCycles> {
    let create_tx = TransactionView {
        hash: format!("0x{}", "f1".repeat(32)),
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
            capacity: "0x342770c00".to_string(),
            lock: fixture_lock_script(),
            type_: None,
        }],
        outputs_data: vec!["0x".to_string()],
        witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
    };

    let split_tx = TransactionView {
        hash: format!("0x{}", "f2".repeat(32)),
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
        outputs: vec![
            CellOutput {
                capacity: "0x1dcd65000".to_string(),
                lock: fixture_lock_script(),
                type_: None,
            },
            CellOutput {
                capacity: "0x165a0bc00".to_string(),
                lock: fixture_lock_script_b(),
                type_: None,
            },
        ],
        outputs_data: vec!["0x".to_string(), "0x".to_string()],
        witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
    };

    let next_day_noop_tx = TransactionView {
        hash: format!("0x{}", "f3".repeat(32)),
        version: "0x0".to_string(),
        cell_deps: vec![],
        header_deps: vec![],
        inputs: vec![CellInput {
            since: "0x0".to_string(),
            previous_output: OutPoint {
                tx_hash: split_tx.hash.clone(),
                index: "0x1".to_string(),
            },
        }],
        outputs: vec![CellOutput {
            capacity: "0x165a0bc00".to_string(),
            lock: fixture_lock_script_b(),
            type_: None,
        }],
        outputs_data: vec!["0x".to_string()],
        witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
    };

    vec![
        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header_with_timestamp(14_003_000, 0xa1, 1_705_276_800_000),
                uncles: vec![],
                transactions: vec![create_tx],
                proposals: vec![],
            },
            cycles: None,
        },
        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header_with_timestamp(14_003_001, 0xa2, 1_705_280_400_000),
                uncles: vec![],
                transactions: vec![split_tx],
                proposals: vec![],
            },
            cycles: None,
        },
        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header_with_timestamp(14_003_002, 0xa3, 1_705_363_200_000),
                uncles: vec![],
                transactions: vec![next_day_noop_tx],
                proposals: vec![],
            },
            cycles: None,
        },
    ]
}

/// Same two 2024-01-15 blocks as `cell_distribution_snapshot_fixture`, but the
/// first block of 2024-01-16 CHANGES the tracked totals (a cellbase minting one
/// new cell, like mainnet block 20,022,562 at 00:00:21 UTC+8). A day-boundary
/// snapshot labelled 2024-01-15 must not contain it.
fn cell_distribution_next_day_mutating_fixture() -> Vec<BlockResponseWithCycles> {
    let mut blocks = cell_distribution_snapshot_fixture();

    let next_day_cellbase = TransactionView {
        hash: format!("0x{}", "f4".repeat(32)),
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
            capacity: "0x342770c00".to_string(),
            lock: fixture_lock_script_with_args(&format!("0x{}", "03".repeat(20))),
            type_: None,
        }],
        outputs_data: vec!["0x".to_string()],
        witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
    };

    let boundary = blocks.last_mut().expect("day-boundary block");
    boundary.block.transactions = vec![next_day_cellbase];
    blocks
}

fn hodl_wave_snapshot_fixture() -> Vec<BlockResponseWithCycles> {
    let mut blocks = hodl_tracker_fixture();
    // Every real block carries a cellbase (blocks 1..=11 on mainnet have one
    // with zero outputs); an outputless cellbase marks the day boundary
    // without touching any balances.
    let boundary_cellbase = TransactionView {
        hash: format!("0x{}", "f7".repeat(32)),
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
        outputs: vec![],
        outputs_data: vec![],
        witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
    };
    blocks.push(BlockResponseWithCycles {
        block: BlockView {
            header: fixture_header_with_timestamp(14_002_002, 0x93, 1_705_363_200_000),
            uncles: vec![],
            transactions: vec![boundary_cellbase],
            proposals: vec![],
        },
        cycles: None,
    });
    blocks
}

/// Same two 2024-01-15 blocks as `hodl_wave_snapshot_fixture`, but the first
/// block of 2024-01-16 mints a cell for a brand-new holder, so folding it into
/// the 2024-01-15 snapshot is visible in both `holder_count` and the age bands.
fn hodl_wave_next_day_mutating_fixture() -> Vec<BlockResponseWithCycles> {
    let mut blocks = hodl_wave_snapshot_fixture();

    let next_day_cellbase = TransactionView {
        hash: format!("0x{}", "f8".repeat(32)),
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
            capacity: "0x342770c00".to_string(),
            lock: fixture_lock_script_with_args(&format!("0x{}", "04".repeat(20))),
            type_: None,
        }],
        outputs_data: vec!["0x".to_string()],
        witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
    };

    let boundary = blocks.last_mut().expect("day-boundary block");
    boundary.block.transactions = vec![next_day_cellbase];
    blocks
}

fn bulk_stage_handoff_fixture() -> Vec<BlockResponseWithCycles> {
    let create_tx = TransactionView {
        hash: format!("0x{}", "d1".repeat(32)),
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
            capacity: "0x342770c00".to_string(),
            lock: fixture_lock_script(),
            type_: None,
        }],
        outputs_data: vec!["0x".to_string()],
        witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
    };

    let split_tx = TransactionView {
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
        outputs: vec![
            CellOutput {
                capacity: "0x1dcd65000".to_string(),
                lock: fixture_lock_script(),
                type_: None,
            },
            CellOutput {
                capacity: "0x165a0bc00".to_string(),
                lock: fixture_lock_script_b(),
                type_: None,
            },
        ],
        outputs_data: vec!["0x".to_string(), "0x".to_string()],
        witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
    };

    let merge_tx = TransactionView {
        hash: format!("0x{}", "d3".repeat(32)),
        version: "0x0".to_string(),
        cell_deps: vec![],
        header_deps: vec![],
        inputs: vec![CellInput {
            since: "0x0".to_string(),
            previous_output: OutPoint {
                tx_hash: split_tx.hash.clone(),
                index: "0x1".to_string(),
            },
        }],
        outputs: vec![CellOutput {
            capacity: "0x165a0bc00".to_string(),
            lock: fixture_lock_script(),
            type_: None,
        }],
        outputs_data: vec!["0x".to_string()],
        witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
    };

    vec![
        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header_with_timestamp(1, 0xb1, 1_705_276_800_000),
                uncles: vec![],
                transactions: vec![create_tx],
                proposals: vec![],
            },
            cycles: None,
        },
        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header_with_timestamp(2, 0xb2, 1_705_280_400_000),
                uncles: vec![],
                transactions: vec![split_tx],
                proposals: vec![],
            },
            cycles: None,
        },
        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header_with_timestamp(3, 0xb3, 1_705_284_000_000),
                uncles: vec![],
                transactions: vec![merge_tx],
                proposals: vec![],
            },
            cycles: None,
        },
    ]
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
    // Cell-by-code index rows carry the script's hash_type: both fixtures use
    // hash_type=type (1), so a bulk-built row must be filed under that form.
    let expected_live_lock_code =
        keys::encode_cell_code_index_key(&lock_code_hash, 1, 14_000_777, &consume_tx_hash, 0);
    let expected_live_type =
        keys::encode_cell_index_key(&type_hash, 14_000_777, &consume_tx_hash, 0);
    let expected_live_type_code =
        keys::encode_cell_code_index_key(&type_code_hash, 1, 14_000_777, &consume_tx_hash, 0);
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

    // The data-form key of the same code hash must not exist: bulk build files
    // each row under exactly one (code_hash, hash_type) form.
    let data_form_lock_code =
        keys::encode_cell_code_index_key(&lock_code_hash, 0, 14_000_777, &consume_tx_hash, 0);
    assert!(!snapshot.cell_by_lock_code.contains(&data_form_lock_code));

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
fn bulk_build_materializes_tx_actions_from_single_pass() {
    let block = same_block_create_then_consume_fixture();
    let consume_tx_hash =
        hex::decode(&block.block.transactions[1].hash[2..]).expect("consume tx hash");
    let lock_hash = ScriptParser::compute_script_hash(&fixture_lock_script());

    let snapshot =
        materialize_bulk_artifacts_for_test(&[block]).expect("bulk build artifact snapshot");

    // Only non-cellbase txs are materialized to CF_TX_ACTIONS.
    // Cellbase (tx index 0) is excluded — the API filters them at read time.
    assert_eq!(snapshot.tx_actions_map.len(), 1);

    let consume_key = keys::encode_tx_actions_key(14_000_321, 1, &consume_tx_hash);

    let consume_actions = snapshot
        .tx_actions_map
        .get(&consume_key)
        .expect("consume tx actions");
    assert!(!consume_actions.is_cellbase);
    assert_eq!(consume_actions.participants.len(), 1);
    assert_eq!(consume_actions.participants[0].lock_hash, lock_hash);
    assert_eq!(consume_actions.participants[0].ckb_delta, 0);
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
fn bulk_build_materializes_bit_cell_as_independent_collection_history() {
    let snapshot = materialize_bulk_artifacts_for_test(&object_activity_fixture())
        .expect("bulk build artifact snapshot");

    let bit_cell_agg = snapshot
        .core
        .object_state
        .bit_cell_agg
        .as_ref()
        .expect(".bit Cell collection aggregate");
    assert_eq!(bit_cell_agg.activities_count, 2);
    assert_eq!(bit_cell_agg.total_count, 1);
    assert_eq!(bit_cell_agg.live_count, 0);
    assert_eq!(bit_cell_agg.holders_count, 0);
    assert_eq!(
        snapshot
            .core
            .object_state
            .identities_by_collection
            .get(BIT_CELL_SENTINEL_COLLECTION.as_slice())
            .expect(".bit Cell collection identities"),
        &vec![
            hex::decode("81d34cd1dfc27716073d1018a63712926d8e3ab36345847129d0cc4135d1ffd4")
                .expect("fixture identity ID")
        ]
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
    assert_eq!(split.tx_actions_map.len(), single.tx_actions_map.len());
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
            .bit_cell_agg
            .as_ref()
            .expect("split .bit Cell aggregate")
            .activities_count,
        single
            .core
            .object_state
            .bit_cell_agg
            .as_ref()
            .expect("single .bit Cell aggregate")
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
fn bulk_build_multi_batch_materialization_matches_single_pass_for_dao_activity_ar_lookups() {
    let blocks = bulk_build_dao_activity_fixture();
    let split_batches = vec![
        vec![blocks[0].clone()],
        vec![blocks[1].clone(), blocks[2].clone()],
    ];
    let completion_tx_hash = vec![0xa3; 32];

    let single =
        materialize_bulk_artifacts_for_test(&blocks).expect("single-pass dao bulk build artifact");
    let split = materialize_bulk_artifacts_from_batches_for_test(&split_batches)
        .expect("multi-batch dao bulk build artifact");

    let dao_withdraw_complete = |snapshot: &ckbadger_indexer::sync::BulkArtifactSnapshot| {
        snapshot
            .tx_actions_map
            .values()
            .find(|actions| actions.tx_hash == completion_tx_hash)
            .and_then(|actions| {
                actions.protocol_actions.iter().find_map(|pa| {
                    if pa.protocol == "dao" && pa.action == "withdraw_complete" {
                        let meta = pa.metadata_value().ok()?;
                        let capacity = meta.get("capacity")?.as_i64()?;
                        let compensation = meta.get("compensation")?.as_i64()?;
                        Some((capacity, compensation))
                    } else {
                        None
                    }
                })
            })
    };

    assert_eq!(
        dao_withdraw_complete(&single),
        Some((200_00000000, 19_60000000))
    );
    assert_eq!(
        dao_withdraw_complete(&split),
        dao_withdraw_complete(&single)
    );
}

#[test]
fn bulk_build_materializes_dao_daily_snapshot_and_latest_stats() {
    let blocks = bulk_build_dao_activity_fixture();
    let snapshot =
        materialize_bulk_artifacts_for_test(&blocks).expect("bulk build dao artifact snapshot");

    let dao_daily = snapshot
        .dao_daily_snapshots
        .values()
        .next()
        .expect("dao daily snapshot");
    assert_eq!(dao_daily.total_deposited, 0);
    assert_eq!(dao_daily.depositors_count, 0);
    assert_eq!(dao_daily.new_deposits, 1);
    assert_eq!(dao_daily.withdrawals, 1);

    let latest = snapshot
        .latest_dao_statistics
        .as_ref()
        .expect("latest dao statistics");
    assert_eq!(latest.tip_block_number, 102);
    assert_eq!(latest.total_deposited, 0);
    assert_eq!(latest.total_depositors, 0);
    assert_eq!(latest.active_deposits, 0);
    assert_eq!(latest.total_compensation_paid, 19_60000000);

    let top = snapshot
        .dao_top_depositors
        .as_ref()
        .expect("dao top depositors");
    assert_eq!(top.tip_block_number, 102);
}

#[test]
fn bulk_build_materializes_script_daily_deltas_from_same_block_recreate() {
    let block = same_block_typed_data_create_then_consume_fixture();
    let lock_code_hash = hex::decode(&fixture_lock_script().code_hash[2..]).expect("lock code");
    let type_code_hash = hex::decode(&fixture_type_script().code_hash[2..]).expect("type code");

    let snapshot =
        materialize_bulk_artifacts_for_test(&[block]).expect("bulk build artifact snapshot");
    let date = snapshot
        .daily_activity_stats
        .keys()
        .next()
        .expect("single date")
        .parse::<u32>()
        .expect("date key");

    let lock_daily = snapshot
        .script_daily_deltas
        .get(&(lock_code_hash.clone(), 1, false))
        .and_then(|timeline| timeline.get(&date))
        .expect("lock daily delta");
    let lock_info = snapshot
        .core
        .script_infos
        .get(&lock_code_hash)
        .expect("lock script info");
    assert_eq!(
        lock_daily.owned_capacity_delta,
        lock_info.lock_owned_capacity_sum
    );
    assert_eq!(
        lock_daily.owned_knowledge_delta,
        lock_info.lock_owned_knowledge_sum
    );

    let type_daily = snapshot
        .script_daily_deltas
        .get(&(type_code_hash.clone(), 1, true))
        .and_then(|timeline| timeline.get(&date))
        .expect("type daily delta");
    let type_info = snapshot
        .core
        .script_infos
        .get(&type_code_hash)
        .expect("type script info");
    assert_eq!(
        type_daily.owned_capacity_delta,
        type_info.type_owned_capacity_sum
    );
    assert_eq!(
        type_daily.owned_knowledge_delta,
        type_info.type_owned_knowledge_sum
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

#[test]
fn bulk_build_materializes_hodl_tracker_state_without_db_reads() {
    let snapshot = materialize_bulk_artifacts_for_test(&hodl_tracker_fixture())
        .expect("hodl tracker snapshot");

    let state = snapshot
        .hodl_tracker_state
        .as_ref()
        .expect("persisted hodl tracker state");
    assert_eq!(state.holder_count, 2);
    assert_eq!(state.last_processed_block, Some(14_002_001));
    assert_eq!(state.capacity_by_date.len(), 1);
    assert_eq!(
        state.capacity_by_date[0],
        ("20240115".to_string(), 140_00000000)
    );
    assert_eq!(state.date_transitions.len(), 1);
    assert_eq!(
        state.date_transitions[0],
        (14_002_000, "20240115".to_string())
    );
}

#[test]
fn bulk_build_materializes_cell_distribution_tracker_state_without_db_reads() {
    let snapshot =
        materialize_bulk_artifacts_for_test(&hodl_tracker_fixture()).expect("cell dist snapshot");

    let state = snapshot
        .cell_dist_tracker_state
        .as_ref()
        .expect("persisted cell distribution tracker state");
    assert_eq!(state.count_by_bucket, [2, 0, 0, 0, 0, 0]);
    assert_eq!(
        state.total_capacity_by_bucket,
        [122_00000000, 0, 0, 0, 0, 0]
    );
    assert_eq!(state.last_processed_block, Some(14_002_001));
    assert_eq!(state.date_transitions.len(), 1);
    assert_eq!(
        state.date_transitions[0],
        (14_002_000, "20240115".to_string())
    );
    assert_eq!(state.cohort_accum.len(), 1);
    assert_eq!(
        state.cohort_accum[0],
        ("2024-01".to_string(), 122_00000000, 140_00000000)
    );
}

#[test]
fn bulk_build_materializes_sealed_cell_distribution_and_address_cohort_on_day_boundary() {
    let snapshot = materialize_bulk_artifacts_for_test(&cell_distribution_snapshot_fixture())
        .expect("cell distribution day-boundary snapshot");

    assert!(
        snapshot.report.sealed_aggregate_rows > 0,
        "bulk build should materialize sealed cell-distribution rows"
    );
    assert_eq!(snapshot.cell_distribution_snapshots.len(), 1);
    assert_eq!(snapshot.address_cohort_snapshots.len(), 1);

    let dist = snapshot
        .cell_distribution_snapshots
        .get("20240115")
        .expect("sealed cell distribution snapshot");
    assert_eq!(dist.size_bucket_counts, [2, 0, 0, 0, 0, 0]);
    assert_eq!(dist.size_bucket_capacities, [122_00000000, 0, 0, 0, 0, 0]);
    assert!(
        !snapshot
            .cell_distribution_snapshots
            .contains_key("20240116"),
        "current in-progress day must not be materialized"
    );

    let cohort = snapshot
        .address_cohort_snapshots
        .get("20240115")
        .expect("sealed address cohort snapshot");
    assert_eq!(cohort.cohorts.len(), 1);
    assert_eq!(cohort.cohorts[0].cohort_month, "2024-01");
    assert_eq!(cohort.cohorts[0].used_capacity, 122_00000000);
    assert_eq!(cohort.cohorts[0].total_balance, 140_00000000);
    assert!(
        !snapshot.address_cohort_snapshots.contains_key("20240116"),
        "current in-progress day must not be materialized"
    );
}

#[test]
fn bulk_build_seals_cell_distribution_before_applying_the_first_block_of_the_next_day() {
    let snapshot =
        materialize_bulk_artifacts_for_test(&cell_distribution_next_day_mutating_fixture())
            .expect("cell distribution day-boundary snapshot");

    let dist = snapshot
        .cell_distribution_snapshots
        .get("20240115")
        .expect("sealed cell distribution snapshot");
    // 2024-01-15 ended with exactly two live cells (80 + 60 CKB, 61 CKB
    // occupied each). The 2024-01-16 cellbase mints a third one; it belongs to
    // the next day's snapshot, never to this one.
    assert_eq!(dist.size_bucket_counts, [2, 0, 0, 0, 0, 0]);
    assert_eq!(dist.size_bucket_capacities, [122_00000000, 0, 0, 0, 0, 0]);

    let cohort = snapshot
        .address_cohort_snapshots
        .get("20240115")
        .expect("sealed address cohort snapshot");
    assert_eq!(cohort.cohorts.len(), 1);
    assert_eq!(cohort.cohorts[0].cohort_month, "2024-01");
    assert_eq!(cohort.cohorts[0].used_capacity, 122_00000000);
    assert_eq!(cohort.cohorts[0].total_balance, 140_00000000);
}

#[test]
fn bulk_build_seals_hodl_wave_before_applying_the_first_block_of_the_next_day() {
    let snapshot = materialize_bulk_artifacts_for_test(&hodl_wave_next_day_mutating_fixture())
        .expect("hodl wave day-boundary snapshot");

    let wave = snapshot
        .hodl_waves
        .get("20240115")
        .expect("sealed hodl wave snapshot");
    // Two holders and 140 CKB, all created on 2024-01-15. The 2024-01-16
    // cellbase adds a third holder whose cell is one day NEWER than the
    // snapshot date, so folding it in would also land it in a nonsense band.
    assert_eq!(wave.holder_count, 2);
    assert_eq!(wave.band_24h, 140_00000000);
    assert_eq!(wave.band_gt_3y, 0);
}

#[test]
fn bulk_build_materializes_sealed_hodl_wave_on_day_boundary() {
    let snapshot = materialize_bulk_artifacts_for_test(&hodl_wave_snapshot_fixture())
        .expect("hodl wave day-boundary snapshot");

    assert!(
        snapshot.report.sealed_aggregate_rows > 0,
        "bulk build should materialize sealed hodl-wave rows"
    );
    assert_eq!(snapshot.hodl_waves.len(), 1);

    let wave = snapshot
        .hodl_waves
        .get("20240115")
        .expect("sealed hodl wave snapshot");
    assert_eq!(wave.band_24h, 140_00000000);
    assert_eq!(wave.band_1d_1w, 0);
    assert_eq!(wave.band_1w_1m, 0);
    assert_eq!(wave.band_1m_3m, 0);
    assert_eq!(wave.band_3m_6m, 0);
    assert_eq!(wave.band_6m_1y, 0);
    assert_eq!(wave.band_1y_3y, 0);
    assert_eq!(wave.band_gt_3y, 0);
    assert_eq!(wave.holder_count, 2);
    assert!(
        !snapshot.hodl_waves.contains_key("20240116"),
        "current in-progress day must not be materialized"
    );
}

#[test]
fn bulk_build_stage_handoff_materializes_consistent_partial_state_without_completion_marker() {
    let snapshot = materialize_bulk_stage_for_test(&bulk_stage_handoff_fixture(), 3, 1)
        .expect("bulk stage handoff snapshot");

    assert_eq!(snapshot.sync_status.tip_block_number, 2);
    assert_eq!(snapshot.sync_status.total_transactions, 2);
    assert_eq!(snapshot.sync_status.total_cells_created, 3);
    assert_eq!(snapshot.sync_status.total_cells_consumed, 1);
    assert!(snapshot.sync_status.sync_started_at.is_some());
    assert!(
        snapshot.sync_status.bulk_sync_completed_at.is_none(),
        "handoff into live pipeline must not mark bulk sync complete early"
    );
    assert_eq!(snapshot.sync_status.bulk_sync_completed_block, None);
    assert!(
        snapshot.bulk_build_session_marker.is_none(),
        "handoff-ready bulk stage must clear the in-progress marker"
    );

    assert_eq!(snapshot.block_headers.len(), 2);
    assert!(snapshot.block_headers.contains_key(&1));
    assert!(snapshot.block_headers.contains_key(&2));
    assert!(
        !snapshot.block_headers.contains_key(&3),
        "blocks reserved for pipeline handoff must remain unmaterialized"
    );

    let lock_a = ScriptParser::compute_script_hash(&fixture_lock_script());
    let lock_b = ScriptParser::compute_script_hash(&fixture_lock_script_b());
    let balance_a = snapshot
        .core
        .address_balances
        .get(&lock_a)
        .expect("address A balance after stage handoff");
    let balance_b = snapshot
        .core
        .address_balances
        .get(&lock_b)
        .expect("address B balance after stage handoff");
    assert_eq!(balance_a.balance, 80_00000000);
    assert_eq!(balance_b.balance, 60_00000000);
}

#[test]
fn bulk_build_stage_handoff_pipeline_completion_marks_final_sync_status_at_chain_tip() {
    let status = materialize_bulk_stage_then_complete_sync_status_for_test(
        &bulk_stage_handoff_fixture(),
        3,
        1,
    )
    .expect("bulk stage handoff pipeline completion status");

    assert_eq!(status.tip_block_number, 3);
    assert_eq!(status.total_transactions, 3);
    assert_eq!(status.total_cells_created, 4);
    assert_eq!(status.total_cells_consumed, 2);
    assert!(status.sync_started_at.is_some());
    assert!(
        status.bulk_sync_completed_at.is_some(),
        "pipeline completion after handoff must persist the bulk completion marker"
    );
    assert_eq!(status.bulk_sync_completed_block, Some(3));
}

#[test]
fn startup_bulk_build_route_runs_handoff_and_pipeline_completion_for_fresh_store() {
    let snapshot =
        simulate_startup_sync_path_for_test(&bulk_stage_handoff_fixture(), 3, 1, 0, None)
            .expect("startup bulk-build route snapshot");

    assert_eq!(snapshot.path, "bulk_build");
    let status = snapshot
        .sync_status
        .expect("fresh-store bulk route must produce a completed sync status");
    assert_eq!(status.tip_block_number, 3);
    assert_eq!(status.total_transactions, 3);
    assert_eq!(status.total_cells_created, 4);
    assert_eq!(status.total_cells_consumed, 2);
    assert!(status.bulk_sync_completed_at.is_some());
    assert_eq!(status.bulk_sync_completed_block, Some(3));
}

#[test]
fn startup_existing_tip_routes_to_pipeline_without_running_bulk_build_test_seam() {
    let snapshot = simulate_startup_sync_path_for_test(
        &bulk_stage_handoff_fixture(),
        3,
        1,
        1,
        Some(vec![0x11; 32]),
    )
    .expect("startup existing-tip route snapshot");

    assert_eq!(snapshot.path, "pipeline");
    assert!(
        snapshot.sync_status.is_none(),
        "non-fresh startup should not run the bulk-build handoff completion seam"
    );
}

#[test]
fn startup_fresh_tip_at_bulk_threshold_routes_to_pipeline_without_running_bulk_build_test_seam() {
    let snapshot =
        simulate_startup_sync_path_for_test(&bulk_stage_handoff_fixture(), 3, 3, 0, None)
            .expect("startup threshold-edge route snapshot");

    assert_eq!(snapshot.path, "pipeline");
    assert!(
        snapshot.sync_status.is_none(),
        "fresh startup at the exact bulk threshold must stay on pipeline path"
    );
}

#[test]
fn startup_route_fail_fast_when_chain_tip_is_behind_sync_tip() {
    let err = simulate_startup_sync_path_for_test(
        &bulk_stage_handoff_fixture(),
        2,
        1,
        3,
        Some(vec![0x22; 32]),
    )
    .unwrap_err();

    assert!(err.to_string().contains("invalid tip ordering"));
    assert!(err.to_string().contains("startup sync path test"));
}

// ============================================================
// B4 regression: bulk build must materialize the chain-level
// hourly buckets (STATS_PREFIX_HOURLY, UTC-keyed) and the miner
// daily buckets (STATS_PREFIX_MINER, UTC+8 date + witness-miner
// lock hash) with semantics identical to the live writer —
// otherwise every rebuild leaves 24h metrics missing history and
// the miner distribution covering only the live-sync segment.
// ============================================================

/// Blocks at UTC 2023-11-18T20:01/20:02/21:01. The UTC hour keys are
/// "2023111820"/"2023111821", while the UTC+8 calendar date of the same
/// instants is 2023-11-19 — so the fixture pins BOTH conventions: hourly
/// buckets keyed on the UTC clock, miner buckets on the UTC+8 date.
fn chain_hourly_and_miner_fixture() -> Vec<BlockResponseWithCycles> {
    let hour1_start_ms: i64 = 1_700_337_600_000; // 2023-11-18T20:00:00Z

    let create_tx = TransactionView {
        hash: format!("0x{}", "a7".repeat(32)),
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
            capacity: "0x2540be400".to_string(), // 10_000_000_000
            lock: fixture_lock_script(),
            type_: None,
        }],
        outputs_data: vec!["0x".to_string()],
        witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
    };
    let consume_tx = TransactionView {
        hash: format!("0x{}", "b7".repeat(32)),
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
            lock: fixture_lock_script_b(),
            type_: None,
        }],
        outputs_data: vec!["0x".to_string()],
        witnesses: vec!["0x".to_string()],
    };
    let cellbase_only = |hash_hex: &str| TransactionView {
        hash: format!("0x{}", hash_hex.repeat(32)),
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
        witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
    };

    vec![
        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header_with_timestamp(100, 0xc1, hour1_start_ms + 60_000),
                uncles: vec![],
                transactions: vec![create_tx, consume_tx],
                proposals: vec![],
            },
            cycles: None,
        },
        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header_with_timestamp(101, 0xc2, hour1_start_ms + 120_000),
                uncles: vec![],
                transactions: vec![cellbase_only("c7")],
                proposals: vec![],
            },
            cycles: None,
        },
        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header_with_timestamp(102, 0xc3, hour1_start_ms + 3_660_000),
                uncles: vec![],
                transactions: vec![cellbase_only("d7")],
                proposals: vec![],
            },
            cycles: None,
        },
    ]
}

#[test]
fn bulk_build_materializes_utc_keyed_chain_hourly_stats() {
    let snapshot = materialize_bulk_artifacts_for_test(&chain_hourly_and_miner_fixture())
        .expect("bulk build artifact snapshot");

    // Live-writer semantics, bit for bit: blocks per bucket, tx count
    // INCLUDING cellbase, per-block cells created/consumed, non-cellbase
    // output capacity as capacity_transferred, HourlyStats.hour = UTC
    // hour-start epoch seconds.
    let hour1 = snapshot
        .hourly_chain_stats
        .get("2023111820")
        .expect("UTC hour bucket 2023111820 must be materialized by bulk build");
    assert_eq!(hour1.hour, 1_700_337_600);
    assert_eq!(hour1.blocks_count, 2);
    assert_eq!(hour1.transactions_count, 3, "tx count includes cellbase");
    assert_eq!(hour1.cells_created, 3);
    assert_eq!(hour1.cells_consumed, 1);
    assert_eq!(hour1.capacity_transferred, 10_000_000_000);

    let hour2 = snapshot
        .hourly_chain_stats
        .get("2023111821")
        .expect("UTC hour bucket 2023111821 must be materialized by bulk build");
    assert_eq!(hour2.hour, 1_700_341_200);
    assert_eq!(hour2.blocks_count, 1);
    assert_eq!(hour2.transactions_count, 1);
    assert_eq!(hour2.cells_created, 1);
    assert_eq!(hour2.cells_consumed, 0);
    assert_eq!(hour2.capacity_transferred, 0);

    // The UTC+8 hour strings for the same instants must NOT appear: chain
    // hourly buckets are UTC-keyed (activity hourly buckets are the UTC+8
    // family).
    assert!(!snapshot.hourly_chain_stats.contains_key("2023111904"));
    assert!(!snapshot.hourly_chain_stats.contains_key("2023111905"));
}

#[test]
fn bulk_build_materializes_miner_stats_from_cellbase_witness() {
    let snapshot = materialize_bulk_artifacts_for_test(&chain_hourly_and_miner_fixture())
        .expect("bulk build artifact snapshot");

    // Miner attribution: the cellbase WITNESS lock (RFC-0022) — the same
    // semantics the live writer uses — keyed by the UTC+8 calendar date
    // ("20231119" for UTC 2023-11-18T20:xx).
    let miner_lock = Script {
        code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8".to_string(),
        hash_type: "type".to_string(),
        args: "0x8211f1b938a107cd53b6302cc752a6fc3965638d".to_string(),
    };
    let miner_hash = ScriptParser::compute_script_hash(&miner_lock);

    let row = snapshot
        .miner_stats
        .get(&("20231119".to_string(), miner_hash.clone()))
        .expect("miner daily bucket must be materialized by bulk build (UTC+8 date key)");
    assert_eq!(row.miner_lock_hash, miner_hash);
    assert_eq!(row.blocks_count, 3);
    assert_eq!(row.last_block_number, 102);

    // The UTC calendar date must NOT be used for the miner key.
    assert!(!snapshot
        .miner_stats
        .keys()
        .any(|(date, _)| date == "20231118"));
}
