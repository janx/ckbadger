use crate::common::*;
use ckb_types::{
    bytes::Bytes,
    core::{BlockBuilder, EpochNumberWithFraction, TransactionBuilder},
    packed,
    prelude::*,
};
use ckbadger_indexer::parser::registry::{ProtocolScript, PROTOCOL_REGISTRY};
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

async fn display_fixture(missing_object: bool, malformed_file: bool) -> (axum::Router, String) {
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
            lock_code_hash: [0x11; 32],
            lock_hash_type: 2,
            lock_args: vec![0x22; 20],
            type_code_hash: (t.as_ref().map(|s| s.code_hash().as_slice().to_vec()))
                .map(|hash| hash.try_into().unwrap()),
            type_hash_type: t.as_ref().map(|_| 2),
            type_args: t.as_ref().map(|s| s.args().raw_data().to_vec()),
            data_size: data[i].len() as i32,
            udt_amount: (i == 2).then_some(amount),
            data_hash: None,
        };
        // The inputs are consumed by this transaction at (7, 1), its outputs by
        // the one seeded at (8, 0).
        batch.put_consumed_cell(&prev_hash, i as i16, &cell, 7, 7, 1);
        batch.put_consumed_cell(&tx_hash, i as i16, &cell, 7, 8, 0);
    }
    seed_tx_at(&mut batch, &[0x99; 32], 8, 0);
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
                metadata_source: Some(ckbadger_store::types::TokenMetadataSource::Label),
                chain_metadata: None,
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
    let (app, url) = display_fixture(false, false).await;
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
    let (app, url) = display_fixture(true, false).await;
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
    let (app, url) = display_fixture(false, true).await;
    let (status, body) = get_json(&app, &url).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert!(
        body["message"].as_str().unwrap().contains("CKBFS"),
        "{body}"
    );
}

/// The type script of `protocol`'s first bundled registry deployment.
fn registered(protocol: ProtocolScript, args: &[u8]) -> packed::Script {
    let code_hash = PROTOCOL_REGISTRY
        .iter()
        .filter(|(_, p)| *p == protocol)
        .min_by_key(|(hash, _)| *hash)
        .unwrap()
        .0;
    packed::Script::new_builder()
        .code_hash(packed::Byte32::from_slice(code_hash).unwrap())
        .hash_type(1u8.into())
        .args(Bytes::copy_from_slice(args).pack())
        .build()
}

/// One cell of a class the frontend's `kindOf` tells apart: its type script
/// and data, and the `cellType` / display kind the transaction detail gives
/// it.
struct CellKind {
    type_script: Option<packed::Script>,
    data: Vec<u8>,
    cell_type: &'static str,
    display_kind: Option<&'static str>,
}

fn every_cell_kind() -> Vec<CellKind> {
    let kind = |type_script, data, cell_type, display_kind| CellKind {
        type_script,
        data,
        cell_type,
        display_kind,
    };
    let mut mnft_id = vec![0x45; 24];
    mnft_id.extend_from_slice(&7u32.to_be_bytes());
    let bit_cell_data = [
        0u32.to_le_bytes().as_slice(),
        &table(&[
            vec![0; 20],
            1_900_000_000u64.to_le_bytes().to_vec(),
            bytes(b"holder.bit"),
        ]),
    ]
    .concat();
    let mut dotbit_data = vec![0; 80];
    dotbit_data[32..52].copy_from_slice(&[0x63; 20]);
    dotbit_data[72..80].copy_from_slice(&1_950_000_000u64.to_le_bytes());
    vec![
        kind(None, vec![], "plain", None),
        kind(
            Some(
                packed::Script::new_builder()
                    .code_hash([0x99; 32].pack())
                    .hash_type(2u8.into())
                    .build(),
            ),
            vec![],
            "typed",
            None,
        ),
        kind(
            Some(registered(ProtocolScript::Dao, &[])),
            vec![0; 8],
            "nervos-dao",
            None,
        ),
        kind(
            Some(registered(ProtocolScript::Sudt, &[0x30; 32])),
            5u128.to_le_bytes().to_vec(),
            "sudt",
            Some("token"),
        ),
        kind(
            Some(registered(ProtocolScript::Xudt, &[0x31; 32])),
            9u128.to_le_bytes().to_vec(),
            "xudt",
            Some("token"),
        ),
        kind(
            Some(registered(ProtocolScript::SporeNft, &[0x41; 32])),
            table(&[bytes(b"text/plain"), bytes(b"hello"), bytes(&[0x40; 32])]),
            "spore",
            Some("object"),
        ),
        kind(
            Some(registered(ProtocolScript::MnftToken, &mnft_id)),
            vec![0; 11],
            "m-nft",
            Some("object"),
        ),
        kind(
            Some(registered(ProtocolScript::DidCkb, &[0x61; 20])),
            vec![],
            "did-ckb",
            Some("identity"),
        ),
        kind(
            Some(registered(ProtocolScript::BitCell, &[0x62; 32])),
            bit_cell_data,
            "bit-cell",
            Some("identity"),
        ),
        kind(
            Some(registered(ProtocolScript::DotbitAccount, &[0x63; 20])),
            dotbit_data,
            "bit-account",
            Some("identity"),
        ),
        // The data names witness 0, the transaction's CKBFS witness.
        kind(
            Some(registered(ProtocolScript::Ckbfs, &[0x71; 32])),
            table(&[
                0u32.to_le_bytes().to_vec(),
                42u32.to_le_bytes().to_vec(),
                bytes(b"text/plain"),
                bytes(b"ownership-proof.json"),
            ]),
            "ckbfs",
            Some("file"),
        ),
    ]
}

/// Block 7: `prev` (7, 0) creates one cell of every kind, `tx` (7, 1) spends
/// them all and creates every kind again, live. So each of `tx`'s inputs and
/// outputs is a cell with its own cell page, and the indexed metadata every
/// display reads is seeded as the indexer writes it — but for the Spore's
/// object row when `missing_spore`. Returns the router and the two
/// transaction hashes.
async fn every_kind_fixture(
    kinds: &[CellKind],
    missing_spore: bool,
) -> (axum::Router, [u8; 32], [u8; 32]) {
    let store = test_store();
    seed_genesis_baseline(&store);
    let lock = packed::Script::new_builder()
        .code_hash([0x11; 32].pack())
        .hash_type(2u8.into())
        .args([0x22; 20].pack())
        .build();
    let outputs: Vec<_> = kinds
        .iter()
        .map(|kind| {
            packed::CellOutput::new_builder()
                .capacity(100_000_000_000u64.pack())
                .lock(lock.clone())
                .type_(kind.type_script.clone().pack())
                .build()
        })
        .collect();
    let outputs_data: Vec<_> = kinds
        .iter()
        .map(|kind| Bytes::copy_from_slice(&kind.data).pack())
        .collect();
    let prev = TransactionBuilder::default()
        .outputs(outputs.clone())
        .outputs_data(outputs_data.clone())
        .build();
    let prev_hash: [u8; 32] = prev.hash().unpack();
    let tx = TransactionBuilder::default()
        .inputs(
            (0..kinds.len())
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
        .outputs(outputs)
        .outputs_data(outputs_data)
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

    let count = i16::try_from(kinds.len()).unwrap();
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        7,
        &CachedBlockHeader {
            hash: block_hash.to_vec(),
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
    for (hash, tx_idx, inputs_count) in [(prev_hash, 0, 0), (tx_hash, 1, count)] {
        batch.put_tx_hash_map(&hash, 7, tx_idx);
        batch.put_tx_index(
            7,
            tx_idx,
            &TxIndexEntry {
                tx_hash: hash,
                is_cellbase: false,
                timestamp: 1_700_000_000_000,
                inputs_count,
                outputs_count: count,
                fee: 0,
                tx_size: 1000,
                cycles: None,
                semantic_tags: 0,
            },
        );
    }
    for (index, kind) in kinds.iter().enumerate() {
        let type_script = kind.type_script.as_ref();
        let cell = LiveCellInfo {
            capacity: 100_000_000_000,
            lock_code_hash: [0x11; 32],
            lock_hash_type: 2,
            lock_args: vec![0x22; 20],
            type_code_hash: type_script.map(|s| s.code_hash().as_slice().try_into().unwrap()),
            type_hash_type: type_script.map(|s| i16::from(s.hash_type().as_slice()[0])),
            type_args: type_script.map(|s| s.args().raw_data().to_vec()),
            data_size: kind.data.len() as i32,
            udt_amount: (kind.display_kind == Some("token"))
                .then(|| u128::from_le_bytes(kind.data.as_slice().try_into().unwrap())),
            data_hash: Some(ckb_hash::blake2b_256(&kind.data)),
        };
        let index = index as i16;
        batch.put_consumed_cell(&prev_hash, index, &cell, 7, 7, 1);
        batch.put_cell(&tx_hash, index, &cell, 7);
    }
    for (kind, standard) in [(&kinds[3], "sudt"), (&kinds[4], "xudt")] {
        let token = kind.type_script.as_ref().unwrap();
        batch.put_token(
            token.calc_script_hash().as_slice(),
            &TokenInfo {
                type_code_hash: token.code_hash().as_slice().to_vec(),
                hash_type: 1,
                type_args: token.args().raw_data().to_vec(),
                standard: standard.into(),
                name: Some(format!("Exact {standard}")),
                symbol: Some(standard.to_uppercase()),
                decimals: Some(8),
                max_supply: None,
                first_seen_block: 7,
                icon_url: None,
                description: None,
                transfers_count: 1,
                metadata_source: Some(ckbadger_store::types::TokenMetadataSource::Label),
                chain_metadata: None,
            },
        );
    }
    let object = |standard, collection_id, name: Option<&str>, extra| ObjectEntry {
        standard,
        collection_id,
        token_id: None,
        owner_lock_hash: None,
        name: name.map(Into::into),
        description: None,
        is_live: true,
        created_at_block: 7,
        created_at_tx: tx_hash.to_vec(),
        extra,
    };
    if !missing_spore {
        batch.put_spore(
            &[0x41; 32],
            &object(
                ObjectStandard::Spore,
                Some(vec![0x40; 32]),
                Some("hello"),
                ObjectExtra::Spore {
                    content_type: "text/plain".into(),
                    content_length: 5,
                    media_profile: SporeMediaProfile {
                        tier: CompositionTier::PureCkb,
                        ..Default::default()
                    },
                },
            ),
        );
    }
    batch.put_spore(
        &[0x40; 32],
        &object(
            ObjectStandard::SporeCluster,
            None,
            Some("Spore group"),
            ObjectExtra::SporeCluster,
        ),
    );
    let mut mnft_id = vec![0x45; 24];
    mnft_id.extend_from_slice(&7u32.to_be_bytes());
    batch.put_mnft(
        &mnft_id,
        &object(
            ObjectStandard::MnftToken,
            Some(vec![0x45; 24]),
            None,
            ObjectExtra::MnftToken {
                token_index: 7,
                characteristic: vec![0; 8],
                configure: 0,
                state: 0,
            },
        ),
    );
    for (id, standard, name, extra) in [
        (
            vec![0x61; 20],
            IdentityStandard::DidCkb,
            None,
            IdentityExtra::DidCkb,
        ),
        (
            vec![0x62; 32],
            IdentityStandard::BitCell,
            Some("holder.bit"),
            IdentityExtra::BitCell {
                account_id: vec![0; 20],
                expired_at: 1_900_000_000,
            },
        ),
        (
            vec![0x63; 20],
            IdentityStandard::DotBit,
            Some("original.bit"),
            IdentityExtra::DotBit {
                expired_at: Some(1_950_000_000),
                registered_at: Some(1),
                status: Some(0),
            },
        ),
    ] {
        batch.put_identity(
            &id,
            &IdentityEntry {
                standard,
                owner_lock_hash: None,
                name: name.map(Into::into),
                is_live: true,
                created_at_block: 7,
                created_at_tx: tx_hash.to_vec(),
                extra,
            },
        );
    }
    batch.commit().unwrap();
    let app = create_router(test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    ))
    .await;
    (app, prev_hash, tx_hash)
}

/// Seal kinds (A0-1): the cell page classes a cell from the same two values
/// the transaction page does. For every kind — plain, an unregistered type,
/// Nervos DAO, sUDT, xUDT, Spore, mNFT, did:ckb, .bit cell, .bit account,
/// CKBFS file — `/cells/{tx}/{i}` serves `typeCellType` and `display` equal
/// to output `i`'s `cellType` and `display` on `/transactions/{tx}/detail`,
/// and the spent cell behind input `i` equal to that input's; the burn
/// marker `cellType` stays absent.
#[tokio::test]
async fn cell_detail_serves_the_transaction_detail_class_of_every_kind() {
    let kinds = every_cell_kind();
    let (app, prev, tx) = every_kind_fixture(&kinds, false).await;
    let (status, detail) =
        get_json(&app, &format!("/transactions/0x{}/detail", hex::encode(tx))).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    for (index, kind) in kinds.iter().enumerate() {
        let output = &detail["outputs"][index];
        let input = &detail["inputs"][index];
        assert_eq!(output["cellType"], kind.cell_type, "output {index}");
        assert_eq!(input["cellType"], kind.cell_type, "input {index}");
        assert_eq!(
            output["display"]["kind"].as_str(),
            kind.display_kind,
            "output {index}: {}",
            output["display"]
        );
        for (side, hash, served, status_word) in [
            ("output", tx, output, "live"),
            ("input", prev, input, "dead"),
        ] {
            let (status, cell) =
                get_json(&app, &format!("/cells/0x{}/{index}", hex::encode(hash))).await;
            assert_eq!(status, StatusCode::OK, "{side} {index}: {cell}");
            assert_eq!(cell["status"], status_word, "{side} {index}");
            assert_eq!(
                cell["typeCellType"], served["cellType"],
                "{side} {index} ({})",
                kind.cell_type
            );
            assert!(
                cell.as_object().unwrap().contains_key("display"),
                "{side} {index}: display is served, null when there is none"
            );
            assert_eq!(
                cell["display"], served["display"],
                "{side} {index} ({})",
                kind.cell_type
            );
            assert!(cell.get("cellType").is_none(), "{side} {index}: {cell}");
        }
    }
    // The display is the transaction tile's whole object, metadata included.
    let (_, token) = get_json(&app, &format!("/cells/0x{}/3", hex::encode(tx))).await;
    assert_eq!(token["display"]["standard"], "sudt");
    assert_eq!(token["display"]["amount"], "5");
    assert_eq!(token["display"]["symbol"], "SUDT");
    let (_, spore) = get_json(&app, &format!("/cells/0x{}/5", hex::encode(tx))).await;
    assert_eq!(spore["display"]["collectionName"], "Spore group");
    let (_, account) = get_json(&app, &format!("/cells/0x{}/9", hex::encode(tx))).await;
    assert_eq!(account["display"]["name"], "original.bit");
}

/// The cell page builds its display by the transaction detail's rule for an
/// indexed cell: a Spore whose object row is missing is an upstream indexer
/// bug — refused by both endpoints, the cell page naming the outpoint —
/// never a cell served without its display. The other kinds still serve.
#[tokio::test]
async fn cell_detail_rejects_a_display_cell_missing_its_indexed_metadata() {
    let kinds = every_cell_kind();
    let (app, _, tx) = every_kind_fixture(&kinds, true).await;
    let tx = hex::encode(tx);
    let (status, detail) = get_json(&app, &format!("/transactions/0x{tx}/detail")).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{detail}");
    let (status, body) = get_json(&app, &format!("/cells/0x{tx}/5")).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    let message = body["message"].as_str().unwrap();
    assert!(
        message.contains(&format!("cell 0x{tx}:5 display"))
            && message.contains("missing indexed object metadata")
            && message.contains(&"41".repeat(32)),
        "{message}"
    );
    let (status, body) = get_json(&app, &format!("/cells/0x{tx}/3")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// A v2 payload holds its whole type script or none: one without its
/// hash_type is refused when it is written, with its outpoint, whatever label
/// its code hash carries, so the transaction-cells view can never be handed one
/// (the store refuses to read one too: `ckbadger-store` `types::tests`).
#[tokio::test]
async fn transaction_cells_input_missing_its_type_hash_type_is_never_stored() {
    let store = test_store();
    let cell = LiveCellInfo {
        capacity: 100_000_000_000,
        lock_code_hash: [0x11; 32],
        lock_hash_type: 2,
        lock_args: vec![0x22; 20],
        type_code_hash: Some([0x33; 32]),
        type_hash_type: None,
        type_args: Some(vec![0x44; 20]),
        data_size: 0,
        udt_amount: None,
        data_hash: None,
    };
    let mut batch = StoreBatch::new(&store);
    let error = batch
        .put_cell_payload_by_outpoint(&[0x4c; 32], 0, &cell)
        .expect_err("a typed payload without its hash_type must not be stored")
        .to_string();
    assert!(
        error.contains("type script is partial")
            && error.contains("hash_type=None")
            && error.contains(&hex::encode([0x4c; 32])),
        "{error}"
    );
    assert!(batch.is_empty(), "nothing staged");
}
