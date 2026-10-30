use crate::common::*;
use ckb_types::{
    bytes::Bytes,
    core::{BlockBuilder, EpochNumberWithFraction, TransactionBuilder},
    packed,
    prelude::*,
};
use ckbadger_store::types::{
    CompositionTier, ObjectEntry, ObjectExtra, ObjectStandard, SporeMediaProfile, TokenInfo,
};

fn table(fields: &[Vec<u8>]) -> Vec<u8> {
    let mut offset = 4 * (fields.len() + 1);
    let total = offset + fields.iter().map(Vec::len).sum::<usize>();
    let mut data = (total as u32).to_le_bytes().to_vec();
    for field in fields {
        data.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += field.len();
    }
    for field in fields {
        data.extend_from_slice(field);
    }
    data
}
fn bytes(data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_le_bytes().to_vec();
    out.extend_from_slice(data);
    out
}

async fn display_fixture(
    missing_object: bool,
    malformed_file: bool,
    missing_hash_type: bool,
) -> (axum::Router, String) {
    let store = test_store();
    seed_genesis_baseline(&store);
    let script = |hash: &str, args: &[u8]| {
        packed::Script::new_builder()
            .code_hash(packed::Byte32::from_slice(&hex::decode(hash).unwrap()).unwrap())
            .hash_type(2u8.into())
            .args(Bytes::copy_from_slice(args).pack())
            .build()
    };
    let lock = script(&"11".repeat(32), &[0x22; 20]);
    let file = script(
        "b5d13ffe0547c78021c01fe24dce2e959a1ed8edbca3cb93dd2e9f57fb56d695",
        &[0x31; 32],
    );
    // Select a real bundled Spore deployment instead of duplicating its hashes.
    let spore_code = ckbadger_indexer::parser::registry::PROTOCOL_REGISTRY
        .iter()
        .filter(|(_, p)| *p == ckbadger_indexer::parser::registry::ProtocolScript::SporeNft)
        .min_by_key(|(hash, _)| *hash)
        .unwrap()
        .0
        .clone();
    let spore = script(&hex::encode(&spore_code), &[0x51; 32]);
    let token_code = ckbadger_indexer::parser::registry::PROTOCOL_REGISTRY
        .iter()
        .filter(|(_, p)| *p == ckbadger_indexer::parser::registry::ProtocolScript::Sudt)
        .min_by_key(|(hash, _)| *hash)
        .unwrap()
        .0
        .clone();
    let token = script(&hex::encode(&token_code), &[0x61; 32]);
    let did_code = ckbadger_indexer::parser::registry::PROTOCOL_REGISTRY
        .iter()
        .filter(|(_, p)| *p == ckbadger_indexer::parser::registry::ProtocolScript::DidCkb)
        .min_by_key(|(hash, _)| *hash)
        .unwrap()
        .0
        .clone();
    let did = script(&hex::encode(did_code), &[0x71; 20]);
    let file_data = if malformed_file {
        vec![1, 2, 3]
    } else {
        table(&[
            0u32.to_le_bytes().to_vec(),
            42u32.to_le_bytes().to_vec(),
            bytes(b"text/plain"),
            bytes(b"ownership-proof.json"),
        ])
    };
    let spore_data = table(&[
        bytes(b"text/plain"),
        bytes(b"btcfs://inscriptioni0"),
        bytes(&[0x41; 32]),
    ]);
    let amount = 900719925474099312345678u128;
    let data = [
        file_data,
        spore_data,
        amount.to_le_bytes().to_vec(),
        vec![],
        vec![],
    ];
    let types = [
        Some(file.clone()),
        Some(spore),
        Some(token.clone()),
        Some(did),
        None,
    ];
    let outputs: Vec<_> = types
        .iter()
        .map(|t| {
            packed::CellOutput::new_builder()
                .capacity(100_000_000_000u64.pack())
                .lock(lock.clone())
                .type_(t.clone().pack())
                .build()
        })
        .collect();
    let prev = TransactionBuilder::default()
        .outputs(outputs.clone())
        .outputs_data(
            data.iter()
                .map(|v| Bytes::copy_from_slice(v).pack())
                .collect::<Vec<_>>(),
        )
        .build();
    let prev_hash: [u8; 32] = prev.hash().unpack();
    let tx = TransactionBuilder::default()
        .inputs(
            (0..outputs.len())
                .map(|i| {
                    packed::CellInput::new_builder()
                        .previous_output(
                            packed::OutPoint::new_builder()
                                .tx_hash(prev.hash())
                                .index((i as u32).pack())
                                .build(),
                        )
                        .build()
                })
                .collect::<Vec<_>>(),
        )
        .outputs(outputs.clone())
        .outputs_data(
            data.iter()
                .map(|v| Bytes::copy_from_slice(v).pack())
                .collect::<Vec<_>>(),
        )
        // The CKBFS output's data names witness 0: the contract only accepts
        // a transaction whose named witness is the file's CKBFS witness.
        .witness(Bytes::from_static(b"CKBFS\x00ownership-proof").pack())
        .build();
    let tx_hash: [u8; 32] = tx.hash().unpack();
    let block = BlockBuilder::default()
        .number(7u64.pack())
        .epoch(EpochNumberWithFraction::new(1, 0, 1800).pack())
        .transaction(prev)
        .transaction(tx)
        .build();
    let block_hash: [u8; 32] = block.hash().unpack();
    let chain = seed_ckb_chain(&[block]);
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        7,
        &CachedBlockHeader {
            hash: block_hash.to_vec(),
            parent_hash: vec![0; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 1,
            epoch_index: 0,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 2,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0x1e083126,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: 1_700_000_000_000,
        },
    );
    batch.put_script_info(
        file.code_hash().as_slice(),
        &ckbadger_store::types::ScriptInfo {
            code_hash: file.code_hash().as_slice().to_vec(),
            hash_type: 1,
            ..Default::default()
        },
    );
    batch.put_tx_hash_map(&tx_hash, 7, 1);
    batch.put_tx_index(
        7,
        1,
        &TxIndexEntry {
            tx_hash,
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 5,
            outputs_count: 5,
            fee: 0,
            tx_size: 1000,
            cycles: None,
            semantic_tags: 0,
        },
    );
    for (i, output) in outputs.iter().enumerate() {
        let t = output.type_().to_opt();
        let cell = LiveCellInfo {
            capacity: 100_000_000_000,
            lock_script_hash: lock.calc_script_hash().as_slice().to_vec(),
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 2,
            lock_args: vec![0x22; 20],
            type_script_hash: t.as_ref().map(|s| s.calc_script_hash().as_slice().to_vec()),
            type_code_hash: t.as_ref().map(|s| s.code_hash().as_slice().to_vec()),
            type_hash_type: t.as_ref().and_then(|_| (!missing_hash_type).then_some(2)),
            type_args: t.as_ref().map(|s| s.args().raw_data().to_vec()),
            data_size: data[i].len() as i32,
            occupied_capacity: 0,
            udt_amount: (i == 2).then_some(amount),
            data_hash: None,
        };
        batch.put_consumed_cell_with_consumer(&prev_hash, i as i16, &cell, 7, 7, Some(&tx_hash));
        batch.put_consumed_cell_with_consumer(&tx_hash, i as i16, &cell, 7, 8, Some(&[0x99; 32]));
    }
    batch.commit().unwrap();
    if !missing_object {
        store
            .put_spore_direct(
                &[0x51; 32],
                &ObjectEntry {
                    standard: ObjectStandard::Spore,
                    collection_id: Some(vec![0x41; 32]),
                    token_id: None,
                    owner_lock_hash: None,
                    name: None,
                    description: None,
                    is_live: false,
                    created_at_block: 1,
                    created_at_tx: vec![0x81; 32],
                    extra: ObjectExtra::Spore {
                        content_type: "text/plain".into(),
                        content_length: b"btcfs://inscriptioni0".len() as i64,
                        media_profile: SporeMediaProfile {
                            tier: CompositionTier::BtcCkb,
                            ..Default::default()
                        },
                    },
                },
            )
            .unwrap();
    }
    store
        .put_spore_direct(
            &[0x41; 32],
            &ObjectEntry {
                standard: ObjectStandard::SporeCluster,
                collection_id: None,
                token_id: None,
                owner_lock_hash: None,
                name: Some("Nervape 3x".into()),
                description: None,
                is_live: false,
                created_at_block: 1,
                created_at_tx: vec![0x82; 32],
                extra: ObjectExtra::SporeCluster,
            },
        )
        .unwrap();
    store
        .put_token_direct(
            token.calc_script_hash().as_slice(),
            &TokenInfo {
                type_code_hash: token_code,
                hash_type: 2,
                type_args: vec![0x61; 32],
                standard: "sudt".into(),
                name: Some("Exact Token".into()),
                symbol: Some("EXACT".into()),
                decimals: Some(8),
                max_supply: None,
                first_seen_block: 1,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        )
        .unwrap();
    let mut identities = StoreBatch::new(store.as_ref());
    identities.put_identity(
        &[0x71; 20],
        &ckbadger_store::types::IdentityEntry {
            standard: ckbadger_store::types::IdentityStandard::DidCkb,
            owner_lock_hash: None,
            name: None,
            is_live: false,
            created_at_block: 1,
            created_at_tx: vec![0x83; 32],
            extra: ckbadger_store::types::IdentityExtra::DidCkb,
        },
    );
    identities.commit().unwrap();
    let app = create_router(test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    ))
    .await;
    (
        app,
        format!("/transactions/0x{}/detail", hex::encode(tx_hash)),
    )
}

#[tokio::test]
async fn transaction_cells_expose_exact_spent_object_token_identity_and_file_evidence() {
    let (app, url) = display_fixture(false, false, false).await;
    let (status, body) = get_json(&app, &url).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["inputs"][0]["type"]["hashType"], "data1");
    assert_eq!(body["outputs"][1]["status"], "consumed");
    assert_eq!(
        body["outputs"][1]["consumedByTx"],
        format!("0x{}", "99".repeat(32))
    );
    for side in ["inputs", "outputs"] {
        assert_eq!(body[side][0]["display"]["kind"], "file", "{body}");
        assert_eq!(body[side][0]["display"]["name"], "ownership-proof.json");
        assert_eq!(body[side][0]["cellType"], "ckbfs");
        assert_eq!(body[side][1]["display"]["collectionName"], "Nervape 3x");
        assert_eq!(body[side][1]["display"]["compositionTier"], "btc_ckb");
        assert_eq!(
            body[side][1]["display"]["id"],
            format!("0x{}", "51".repeat(32))
        );
        assert!(body[side][1]["display"]["name"].is_null());
        assert_eq!(
            body[side][2]["display"]["amount"],
            "900719925474099312345678"
        );
        assert_eq!(body[side][2]["display"]["symbol"], "EXACT");
        assert_eq!(body[side][2]["display"]["decimals"], 8);
        assert_eq!(body[side][3]["display"]["standard"], "did_ckb");
        assert_eq!(
            body[side][3]["display"]["id"],
            format!("0x{}", "71".repeat(20))
        );
        assert!(body[side][3]["display"]["name"].is_null());
        assert!(body[side][4]["display"].is_null());
    }
    assert_eq!(
        body["inputs"][0]["display"]["id"],
        body["outputs"][0]["display"]["id"]
    );
}
#[tokio::test]
async fn transaction_cells_reject_missing_indexed_object_metadata() {
    let (app, url) = display_fixture(true, false, false).await;
    let (status, body) = get_json(&app, &url).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    let message = body["message"].as_str().unwrap();
    assert!(
        message.contains("object")
            && message.contains(&"51".repeat(32))
            && message.contains("input"),
        "{message}"
    );
}
#[tokio::test]
async fn transaction_cells_reject_malformed_registered_file_data() {
    let (app, url) = display_fixture(false, true, false).await;
    let (status, body) = get_json(&app, &url).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert!(
        body["message"].as_str().unwrap().contains("CKBFS"),
        "{body}"
    );
}

#[tokio::test]
async fn transaction_cells_reject_missing_immutable_type_hash_type_despite_label() {
    let (app, url) = display_fixture(false, false, true).await;
    let (status, body) = get_json(&app, &url).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    let message = body["message"].as_str().unwrap();
    assert!(
        message.contains("stored type hash_type") && message.contains("prev_outpoint"),
        "{message}"
    );
}
