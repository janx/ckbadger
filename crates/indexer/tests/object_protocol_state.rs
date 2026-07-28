use ckbadger_indexer::parser::bit_cell::BIT_CELL_CODE_HASH_TESTNET;
use ckbadger_indexer::parser::dotbit::DOTBIT_ACCOUNT_CELL_TYPE_ID_TESTNET;
use ckbadger_indexer::parser::mnft::{
    MNFT_CLASS_CODE_HASH, MNFT_ISSUER_CODE_HASH, MNFT_TOKEN_CODE_HASH,
};
use ckbadger_indexer::parser::spore::{CLUSTER_CODE_HASH_MAINNET_V2, SPORE_CODE_HASH_MAINNET_V2};
use ckbadger_indexer::parser::ScriptParser;
use ckbadger_indexer::rpc::{
    BlockResponseWithCycles, BlockView, CellInput, CellOutput, HeaderView, OutPoint, Script,
    TransactionView,
};
use ckbadger_indexer::sync::{materialize_object_state_for_test, ObjectStateSnapshot};
use ckbadger_store::types::{
    IdentityExtra, IdentityStandard, ObjectStandard, BIT_CELL_SENTINEL_COLLECTION,
    DOTBIT_SENTINEL_COLLECTION,
};

/// Real mainnet cellbase first witness (block 12,000,000): block parsing
/// requires every non-genesis cellbase to carry a valid RFC-0022
/// `CellbaseWitness`.
const TEST_CELLBASE_WITNESS: &str = "0x7a0000000c00000055000000490000001000000030000000310000009bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce801140000008211f1b938a107cd53b6302cc752a6fc3965638d210000000000000020302e3131332e3020283832383731613320323032342d30312d303929";

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

fn encode_molecule_table(fields: &[Vec<u8>]) -> Vec<u8> {
    let header_size = 4 + fields.len() * 4;
    let total_size = header_size + fields.iter().map(Vec::len).sum::<usize>();
    let mut result = Vec::with_capacity(total_size);
    result.extend_from_slice(&(total_size as u32).to_le_bytes());

    let mut offset = header_size as u32;
    for field in fields {
        result.extend_from_slice(&offset.to_le_bytes());
        offset += field.len() as u32;
    }
    for field in fields {
        result.extend_from_slice(field);
    }
    result
}

fn encode_molecule_dynvec(items: &[Vec<u8>]) -> Vec<u8> {
    if items.is_empty() {
        return 4u32.to_le_bytes().to_vec();
    }

    encode_molecule_table(items)
}

fn encode_dotbit_account_cell_witness(account_id: &[u8; 20], account: &str) -> String {
    let account_items = account
        .strip_suffix(".bit")
        .unwrap_or(account)
        .chars()
        .map(|character| {
            encode_molecule_table(&[
                2u32.to_le_bytes().to_vec(),
                encode_molecule_bytes(character.to_string().as_bytes()),
            ])
        })
        .collect::<Vec<_>>();
    let entity = encode_molecule_table(&[
        account_id.to_vec(),
        encode_molecule_dynvec(&account_items),
        0u64.to_le_bytes().to_vec(),
        0u64.to_le_bytes().to_vec(),
        0u64.to_le_bytes().to_vec(),
        0u64.to_le_bytes().to_vec(),
        vec![0],
        encode_molecule_dynvec(&[]),
        vec![0],
        0u64.to_le_bytes().to_vec(),
    ]);
    let data_entity = encode_molecule_table(&[
        0u32.to_le_bytes().to_vec(),
        3u32.to_le_bytes().to_vec(),
        encode_molecule_bytes(&entity),
    ]);
    let data = encode_molecule_table(&[Vec::new(), Vec::new(), data_entity]);

    let mut witness = b"das".to_vec();
    witness.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    witness.extend_from_slice(&data);
    format!("0x{}", hex::encode(witness))
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

fn create_testnet_dotbit_account_type_script() -> Script {
    Script {
        code_hash: DOTBIT_ACCOUNT_CELL_TYPE_ID_TESTNET.to_string(),
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

fn create_mnft_issuer_type_script(type_id_hex: &str) -> Script {
    Script {
        code_hash: MNFT_ISSUER_CODE_HASH.to_string(),
        hash_type: "type".to_string(),
        args: type_id_hex.to_string(),
    }
}

fn create_mnft_class_type_script(issuer_id: &[u8; 20], class_index: u32) -> Script {
    let mut args = issuer_id.to_vec();
    args.extend_from_slice(&class_index.to_le_bytes());
    Script {
        code_hash: MNFT_CLASS_CODE_HASH.to_string(),
        hash_type: "type".to_string(),
        args: format!("0x{}", hex::encode(args)),
    }
}

fn create_mnft_token_type_script(class_id: &[u8], token_index: u32) -> Script {
    let mut args = class_id.to_vec();
    args.extend_from_slice(&token_index.to_le_bytes());
    Script {
        code_hash: MNFT_TOKEN_CODE_HASH.to_string(),
        hash_type: "type".to_string(),
        args: format!("0x{}", hex::encode(args)),
    }
}

fn create_mnft_issuer_data(class_count: u32, set_count: u32, info: Option<&str>) -> Vec<u8> {
    let mut data = vec![0u8];
    data.extend_from_slice(&class_count.to_be_bytes());
    data.extend_from_slice(&set_count.to_be_bytes());
    if let Some(info) = info {
        data.extend_from_slice(&(info.len() as u16).to_be_bytes());
        data.extend_from_slice(info.as_bytes());
    }
    data
}

fn create_mnft_class_data(
    total: u32,
    issued: u32,
    configure: u8,
    name: &str,
    description: &str,
) -> Vec<u8> {
    let mut data = vec![0u8];
    data.extend_from_slice(&total.to_be_bytes());
    data.extend_from_slice(&issued.to_be_bytes());
    data.push(configure);
    data.extend_from_slice(&(name.len() as u16).to_be_bytes());
    data.extend_from_slice(name.as_bytes());
    data.extend_from_slice(&(description.len() as u16).to_be_bytes());
    data.extend_from_slice(description.as_bytes());
    data.extend_from_slice(&0u16.to_be_bytes());
    data
}

fn create_mnft_token_data(characteristic: &[u8; 8], configure: u8, state: u8) -> Vec<u8> {
    let mut data = vec![0u8];
    data.extend_from_slice(characteristic);
    data.push(configure);
    data.push(state);
    data
}

fn bulk_build_object_fixture() -> Vec<BlockResponseWithCycles> {
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
                capacity: format!("0x{:x}", 200_00000000u64),
                lock: fixture_lock_script(&format!("0x{}", "03".repeat(20))),
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
            lock: fixture_lock_script(&format!("0x{}", "09".repeat(20))),
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

fn bulk_build_cluster_update_fixture() -> Vec<BlockResponseWithCycles> {
    let cluster_id = [0x44; 32];
    let owner_lock = fixture_lock_script(&format!("0x{}", "0c".repeat(20)));

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
            capacity: format!("0x{:x}", 200_00000000u64),
            lock: owner_lock.clone(),
            type_: Some(create_cluster_type_script(&cluster_id)),
        }],
        outputs_data: vec![format!(
            "0x{}",
            hex::encode(create_cluster_data(
                "Genesis Cluster",
                "{\"dob\":{\"ver\":1}}"
            ))
        )],
        witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
    };

    let update_tx = TransactionView {
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
        outputs: vec![CellOutput {
            capacity: format!("0x{:x}", 200_00000000u64),
            lock: owner_lock,
            type_: Some(create_cluster_type_script(&cluster_id)),
        }],
        outputs_data: vec![format!(
            "0x{}",
            hex::encode(create_cluster_data(
                "Upgraded Cluster",
                "{\"dob\":{\"ver\":2}}"
            ))
        )],
        witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
    };

    vec![
        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header(14_001_100, 0xa1, 1_700_001_000_000),
                uncles: vec![],
                transactions: vec![create_tx],
                proposals: vec![],
            },
            cycles: None,
        },
        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header(14_001_101, 0xa2, 1_700_001_010_000),
                uncles: vec![],
                transactions: vec![update_tx],
                proposals: vec![],
            },
            cycles: None,
        },
    ]
}

fn bulk_build_mnft_object_fixture() -> Vec<BlockResponseWithCycles> {
    let issuer_id = [0x44; 20];
    let mut class_id = issuer_id.to_vec();
    class_id.extend_from_slice(&7u32.to_le_bytes());
    let mut token_id = class_id.clone();
    token_id.extend_from_slice(&9u32.to_le_bytes());
    let issuer_type_id = format!("0x{}", "ab".repeat(32));

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
                capacity: format!("0x{:x}", 250_00000000u64),
                lock: fixture_lock_script(&format!("0x{}", "0a".repeat(20))),
                type_: Some(create_mnft_issuer_type_script(&issuer_type_id)),
            },
            CellOutput {
                capacity: format!("0x{:x}", 260_00000000u64),
                lock: fixture_lock_script(&format!("0x{}", "0a".repeat(20))),
                type_: Some(create_mnft_class_type_script(&issuer_id, 7)),
            },
            CellOutput {
                capacity: format!("0x{:x}", 270_00000000u64),
                lock: fixture_lock_script(&format!("0x{}", "0a".repeat(20))),
                type_: Some(create_mnft_token_type_script(&class_id, 9)),
            },
        ],
        outputs_data: vec![
            format!(
                "0x{}",
                hex::encode(create_mnft_issuer_data(
                    1,
                    0,
                    Some(r#"{"name":"Test Issuer"}"#)
                ))
            ),
            format!(
                "0x{}",
                hex::encode(create_mnft_class_data(
                    100,
                    1,
                    3,
                    "Genesis Class",
                    "class description"
                ))
            ),
            format!(
                "0x{}",
                hex::encode(create_mnft_token_data(&[1, 2, 3, 4, 5, 6, 7, 8], 1, 0))
            ),
        ],
        witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
    };

    let transfer_tx = TransactionView {
        hash: format!("0x{}", "d1".repeat(32)),
        version: "0x0".to_string(),
        cell_deps: vec![],
        header_deps: vec![],
        inputs: vec![CellInput {
            since: "0x0".to_string(),
            previous_output: OutPoint {
                tx_hash: create_tx.hash.clone(),
                index: "0x2".to_string(),
            },
        }],
        outputs: vec![CellOutput {
            capacity: format!("0x{:x}", 270_00000000u64),
            lock: fixture_lock_script(&format!("0x{}", "0b".repeat(20))),
            type_: Some(create_mnft_token_type_script(&class_id, 9)),
        }],
        outputs_data: vec![format!(
            "0x{}",
            hex::encode(create_mnft_token_data(&[1, 2, 3, 4, 5, 6, 7, 8], 1, 0))
        )],
        witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
    };

    let consume_tx = TransactionView {
        hash: format!("0x{}", "e1".repeat(32)),
        version: "0x0".to_string(),
        cell_deps: vec![],
        header_deps: vec![],
        inputs: vec![CellInput {
            since: "0x0".to_string(),
            previous_output: OutPoint {
                tx_hash: transfer_tx.hash.clone(),
                index: "0x0".to_string(),
            },
        }],
        outputs: vec![],
        outputs_data: vec![],
        witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
    };

    vec![
        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header(14_002_000, 0x91, 1_700_100_000_000),
                uncles: vec![],
                transactions: vec![create_tx],
                proposals: vec![],
            },
            cycles: None,
        },
        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header(14_002_001, 0x92, 1_700_100_010_000),
                uncles: vec![],
                transactions: vec![transfer_tx],
                proposals: vec![],
            },
            cycles: None,
        },
        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header(14_002_002, 0x93, 1_700_100_020_000),
                uncles: vec![],
                transactions: vec![consume_tx],
                proposals: vec![],
            },
            cycles: None,
        },
    ]
}

fn failed_testnet_transaction_fixture_with_account_and_bit_cell() -> Vec<BlockResponseWithCycles> {
    let account_id: [u8; 20] = hex::decode("81d34cd1dfc27716073d1018a63712926d8e3ab3")
        .unwrap()
        .try_into()
        .unwrap();
    let tx = TransactionView {
        hash: "0xccef03c785caba4144d106b98e87f8bab2dedbb850dd8002356ab6eba5d572be"
            .to_string(),
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
                lock: fixture_lock_script(&format!("0x{}", "03".repeat(20))),
                type_: Some(create_testnet_dotbit_account_type_script()),
            },
            CellOutput {
                capacity: format!("0x{:x}", 200_00000000u64),
                lock: fixture_lock_script(&format!("0x{}", "03".repeat(20))),
                type_: Some(create_bit_cell_type_script()),
            },
        ],
        outputs_data: vec![
            "0x7e66fb709ae616f18452d6dc08221bb9fba7b270bdc7615e80b41597ca9bbde281d34cd1dfc27716073d1018a63712926d8e3ab3828195525add4b66d20445678f6dd6ef5ba877151b46fc690000000032303234303530372e626974".to_string(),
            "0x000000003c00000010000000240000002c000000a7d4860aaf1dc83daedf75d6022811d2c2ae250b1b46fc69000000000c00000032303234303530372e626974".to_string(),
        ],
        // witnesses[0] must be a valid CellbaseWitness (block parsing derives
        // the miner from it); the das action witness is position-independent.
        witnesses: vec![
            TEST_CELLBASE_WITNESS.to_string(),
            encode_dotbit_account_cell_witness(&account_id, "20240507.bit"),
        ],
    };

    vec![BlockResponseWithCycles {
        block: BlockView {
            header: fixture_header(13_184_726, 0xc9, 1_741_000_000_000),
            uncles: vec![],
            transactions: vec![tx],
            proposals: vec![],
        },
        cycles: None,
    }]
}

#[test]
fn bulk_build_keeps_simultaneous_account_cell_and_bit_cell_lifecycles_separate() {
    let account_id = hex::decode("81d34cd1dfc27716073d1018a63712926d8e3ab3").expect("account ID");
    let bit_cell_id =
        hex::decode("81d34cd1dfc27716073d1018a63712926d8e3ab36345847129d0cc4135d1ffd4")
            .expect(".bit Cell identity ID");
    let snapshot = materialize_object_state_for_test(
        &failed_testnet_transaction_fixture_with_account_and_bit_cell(),
    )
    .expect("both protocol identities must materialize");

    assert_eq!(
        snapshot.identities.len(),
        2,
        "simultaneous identity rows: {:?}",
        snapshot.identities
    );
    assert_eq!(
        snapshot.identities[account_id.as_slice()].standard,
        IdentityStandard::DotBit
    );
    let bit_cell = &snapshot.identities[bit_cell_id.as_slice()];
    assert_eq!(bit_cell.standard, IdentityStandard::BitCell);
    assert!(matches!(
        &bit_cell.extra,
        IdentityExtra::BitCell {
            account_id: linked_account_id,
            expired_at: 1_778_140_699
        } if linked_account_id == &account_id
    ));
    assert_eq!(
        snapshot
            .dotbit_agg
            .expect("AccountCell aggregate")
            .total_count,
        1
    );
    assert_eq!(
        snapshot
            .bit_cell_agg
            .expect(".bit Cell aggregate")
            .total_count,
        1
    );
    assert_eq!(
        snapshot.identities_by_collection[DOTBIT_SENTINEL_COLLECTION.as_slice()],
        vec![account_id]
    );
    assert_eq!(
        snapshot.identities_by_collection[BIT_CELL_SENTINEL_COLLECTION.as_slice()],
        vec![bit_cell_id]
    );
}

#[test]
fn bulk_build_object_owner_materializes_spore_cluster_and_bit_cell_state_without_db_reads() {
    let cluster_id = [0x11; 32];
    let spore_id = [0x22; 32];
    let bit_identity_id =
        hex::decode("81d34cd1dfc27716073d1018a63712926d8e3ab36345847129d0cc4135d1ffd4")
            .expect("fixture identity ID");
    let lock_a_hash =
        ScriptParser::compute_script_hash(&fixture_lock_script(&format!("0x{}", "01".repeat(20))));
    let lock_b_hash =
        ScriptParser::compute_script_hash(&fixture_lock_script(&format!("0x{}", "02".repeat(20))));
    let create_tx_hash = vec![0xa1; 32];
    let transfer_tx_hash = vec![0xb1; 32];
    let spore_type_hash = ScriptParser::compute_script_hash(&create_spore_type_script(&spore_id));

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

    let bit_entry = snapshot
        .identities
        .get(bit_identity_id.as_slice())
        .expect(".bit Cell identity entry exists");
    assert_eq!(bit_entry.standard, IdentityStandard::BitCell);
    assert_eq!(bit_entry.name.as_deref(), Some("20240507.bit"));
    assert!(matches!(
        &bit_entry.extra,
        IdentityExtra::BitCell {
            account_id,
            expired_at: 1_778_140_699
        } if account_id == &hex::decode("81d34cd1dfc27716073d1018a63712926d8e3ab3")
            .expect("fixture account ID")
    ));
    assert!(!bit_entry.is_live);
    assert!(bit_entry.owner_lock_hash.is_none());

    let bit_cell_agg = snapshot.bit_cell_agg.expect(".bit Cell aggregate exists");
    assert_eq!(bit_cell_agg.total_count, 1);
    assert_eq!(bit_cell_agg.live_count, 0);
    assert_eq!(bit_cell_agg.holders_count, 0);

    assert_eq!(
        snapshot
            .identities_by_collection
            .get(BIT_CELL_SENTINEL_COLLECTION.as_slice())
            .expect(".bit Cell collection ids"),
        &vec![bit_identity_id]
    );
    assert!(snapshot.bit_cell_owner_counts.is_empty());

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

#[test]
fn bulk_build_object_owner_materializes_spore_and_cluster_daily_deltas() {
    let cluster_id = [0x11; 32];
    let spore_id = [0x22; 32];

    let snapshot: ObjectStateSnapshot =
        materialize_object_state_for_test(&bulk_build_object_fixture()).expect("object snapshot");

    let spore_daily = snapshot
        .spore_daily_deltas
        .get(spore_id.as_slice())
        .expect("spore daily deltas exist");
    assert_eq!(spore_daily.len(), 1);
    let spore_delta = spore_daily.values().next().expect("single spore delta");
    assert_eq!(spore_delta.owned_capacity_delta, 200_00000000);
    assert!(spore_delta.owned_knowledge_delta > 0);

    let cluster_daily = snapshot
        .cluster_daily_deltas
        .get(cluster_id.as_slice())
        .expect("cluster daily deltas exist");
    assert_eq!(cluster_daily.len(), 1);
    let cluster_delta = cluster_daily.values().next().expect("single cluster delta");
    assert_eq!(cluster_delta.owned_capacity_delta, 200_00000000);
    assert!(cluster_delta.owned_knowledge_delta > 0);
}

#[test]
fn bulk_build_object_owner_updates_cluster_cells_without_crashing() {
    let cluster_id = [0x44; 32];

    let snapshot =
        materialize_object_state_for_test(&bulk_build_cluster_update_fixture()).expect("snapshot");

    let stored_cluster = snapshot
        .spores
        .get(cluster_id.as_slice())
        .expect("cluster entry exists");
    assert_eq!(stored_cluster.standard, ObjectStandard::SporeCluster);
    assert!(stored_cluster.is_live);
    assert_eq!(stored_cluster.name.as_deref(), Some("Upgraded Cluster"));
    assert_eq!(
        stored_cluster.description.as_deref(),
        Some("{\"dob\":{\"ver\":2}}")
    );

    let cluster_agg = snapshot
        .cluster_aggs
        .get(cluster_id.as_slice())
        .expect("cluster aggregate exists");
    assert_eq!(cluster_agg.name.as_deref(), Some("Upgraded Cluster"));
    assert_eq!(
        cluster_agg.description.as_deref(),
        Some("{\"dob\":{\"ver\":2}}")
    );
    assert_eq!(cluster_agg.total_count, 0);
    assert_eq!(cluster_agg.live_count, 0);
}

#[test]
fn bulk_build_object_owner_materializes_mnft_state_without_db_reads() {
    let class_issuer_id = [0x44; 20];
    let mut class_id = class_issuer_id.to_vec();
    class_id.extend_from_slice(&7u32.to_le_bytes());
    let mut token_id = class_id.clone();
    token_id.extend_from_slice(&9u32.to_le_bytes());
    let issuer_type_id = format!("0x{}", "ab".repeat(32));
    let issuer_object_id =
        ScriptParser::compute_script_hash(&create_mnft_issuer_type_script(&issuer_type_id))[..20]
            .to_vec();
    let owner_a_hash =
        ScriptParser::compute_script_hash(&fixture_lock_script(&format!("0x{}", "0a".repeat(20))));
    let issuer_tx_hash = vec![0xc1; 32];
    let transfer_tx_hash = vec![0xd1; 32];
    let token_type_hash =
        ScriptParser::compute_script_hash(&create_mnft_token_type_script(&class_id, 9));
    let transfer_hour_bucket = 1_700_100_010_000_i64 / 3_600_000;

    let snapshot = materialize_object_state_for_test(&bulk_build_mnft_object_fixture())
        .expect("mnft object snapshot");

    let issuer = snapshot
        .objects
        .get(issuer_object_id.as_slice())
        .expect("issuer entry exists");
    assert_eq!(issuer.standard, ObjectStandard::MnftIssuer);
    assert!(issuer.is_live);
    assert_eq!(issuer.owner_lock_hash, Some(owner_a_hash.clone()));

    let class = snapshot
        .objects
        .get(class_id.as_slice())
        .expect("class entry exists");
    assert_eq!(class.standard, ObjectStandard::MnftClass);
    assert!(class.is_live);
    assert_eq!(class.collection_id, Some(class_issuer_id.to_vec()));

    let token = snapshot
        .objects
        .get(token_id.as_slice())
        .expect("token entry exists");
    assert_eq!(token.standard, ObjectStandard::MnftToken);
    assert!(!token.is_live);
    assert!(token.owner_lock_hash.is_none());
    assert_eq!(token.created_at_tx, issuer_tx_hash.clone());

    let class_agg = snapshot
        .object_collection_aggs
        .get(class_id.as_slice())
        .expect("class aggregate exists");
    assert_eq!(class_agg.standard, ObjectStandard::MnftClass);
    assert_eq!(class_agg.name.as_deref(), Some("Genesis Class"));
    assert_eq!(class_agg.total_count, 1);
    assert_eq!(class_agg.live_count, 0);
    assert_eq!(class_agg.holders_count, 0);

    assert_eq!(
        snapshot
            .objects_by_collection
            .get(class_id.as_slice())
            .expect("class members"),
        &vec![token_id.clone()]
    );
    assert!(!snapshot
        .object_owner_counts
        .contains_key(class_id.as_slice()));

    let class_outpoints = snapshot
        .mnft_class_outpoints
        .get(class_id.as_slice())
        .expect("class outpoints");
    assert_eq!(class_outpoints, &vec![(issuer_tx_hash.clone(), 1_i16)]);

    let token_outpoints = snapshot
        .mnft_token_outpoints
        .get(token_id.as_slice())
        .expect("token outpoints");
    assert_eq!(
        token_outpoints,
        &vec![(issuer_tx_hash, 2_i16), (transfer_tx_hash, 0_i16)]
    );

    let type_index = snapshot
        .object_type_indexes
        .get(token_type_hash.as_slice())
        .expect("token type index exists");
    assert_eq!(type_index.collection_id, class_id);

    let hourly = snapshot
        .object_hourly_transfers
        .get(type_index.collection_id.as_slice())
        .expect("hourly transfer stats exist");
    assert_eq!(hourly.get(&transfer_hour_bucket), Some(&1));
}
