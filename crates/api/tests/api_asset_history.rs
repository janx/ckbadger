mod common;
use common::*;

fn capture_producer(name: &str, value: &serde_json::Value) {
    if let Some(dir) = std::env::var_os("CKBADGER_D4_PRODUCER_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{name}.json")),
            serde_json::to_vec_pretty(value).unwrap(),
        )
        .unwrap();
    }
}

fn seed_tx(batch: &mut StoreBatch<'_>, hash: &[u8], block: i64, index: i32) {
    batch.put_tx_hash_map(hash, block, index);
    batch.put_tx_index(
        block,
        index,
        &TxIndexEntry {
            tx_hash: hash.try_into().unwrap(),
            is_cellbase: false,
            timestamp: 1_700_000_000_000 + block,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 200,
            cycles: None,
            semantic_tags: 0,
        },
    );
}

fn cell(id: &[u8], capacity: i64, owner: u8) -> LiveCellInfo {
    let code = vec![0x41; 32];
    let args = vec![owner; 20];
    let type_code = vec![0x42; 32];
    LiveCellInfo {
        capacity,
        occupied_capacity: 114_00000000,
        lock_script_hash: compute_script_hash(&code, 1, &args),
        lock_code_hash: code,
        lock_hash_type: 1,
        lock_args: args,
        type_script_hash: Some(compute_script_hash(&type_code, 1, id)),
        type_code_hash: Some(type_code),
        type_hash_type: Some(1),
        type_args: Some(id.to_vec()),
        data_size: 0,
        udt_amount: None,
        data_hash: Some(vec![0; 32]),
    }
}

fn history_fixture() -> (Arc<CkbadgerStore>, Vec<u8>) {
    history_fixture_with_corrupt_script(None)
}

fn history_fixture_with_corrupt_script(corrupt: Option<&str>) -> (Arc<CkbadgerStore>, Vec<u8>) {
    let store = test_store();
    let id = vec![0x44; 20];
    let mint = vec![0x51; 32];
    let transfer = vec![0x52; 32];
    let orphan = vec![0x53; 32];
    let mut batch = StoreBatch::new(&store);
    batch.put_identity(
        &id,
        &IdentityEntry {
            standard: IdentityStandard::DidCkb,
            owner_lock_hash: Some(cell(&id, 320_00000000, 2).lock_script_hash),
            name: None,
            is_live: true,
            created_at_block: 100,
            created_at_tx: mint.clone(),
            extra: IdentityExtra::DidCkb,
        },
    );
    batch.put_spore_outpoint(&mint, 0, &id);
    batch.put_spore_outpoint(&transfer, 0, &id);
    batch.put_consumed_cell_with_consumer(
        &mint,
        0,
        &cell(&id, 307_00000000, 1),
        100,
        100,
        Some(&transfer),
    );
    let mut current = cell(&id, 320_00000000, 2);
    match corrupt {
        Some("lock") => current.lock_args[0] ^= 1,
        Some("type") => current.type_args.as_mut().unwrap()[0] ^= 1,
        None => {}
        Some(other) => panic!("invalid fixture corruption {other}"),
    }
    batch.put_cell(&transfer, 0, &current, 100);
    seed_tx(&mut batch, &mint, 100, 1);
    seed_tx(&mut batch, &transfer, 100, 2);
    // Immutable payload left by a reorg has no canonical reverse index/marker.
    batch.put_cell_payload_by_outpoint(&orphan, 0, &cell(&id, 999_00000000, 3));
    batch.commit().unwrap();
    (store, id)
}

#[tokio::test]
async fn cell_versions_exact_same_block_order_capacity_and_current_owner() {
    let (store, id) = history_fixture();
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(
        &app,
        &format!(
            "/assets/identities/did/items/0x{}/cell-versions",
            hex::encode(&id)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    capture_producer("did-cell-versions", &json);
    assert_eq!(json["total"], 2);
    assert_eq!(json["data"][0]["capacity"], "30700000000");
    assert_eq!(json["data"][1]["capacity"], "32000000000");
    assert_eq!(json["data"][0]["txIndex"], 1);
    assert_eq!(json["data"][1]["txIndex"], 2);
    assert_eq!(json["data"][0]["isLive"], false);
    assert_eq!(json["data"][1]["isLive"], true);
    assert!(json["data"][1]["ownerAddress"]
        .as_str()
        .unwrap()
        .starts_with("ckb1"));
    assert_eq!(
        json["data"][1]["anatomy"],
        serde_json::json!({"capacityBytes":8,"lockBytes":53,"typeBytes":53,"dataBytes":0})
    );
}

#[tokio::test]
async fn did_detail_has_current_canonical_outpoint() {
    let (store, id) = history_fixture();
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(
        &app,
        &format!("/assets/identities/did/items/0x{}", hex::encode(&id)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["txHash"], format!("0x{}", hex::encode([0x52; 32])));
    assert_eq!(json["outputIndex"], 0);
}

#[tokio::test]
async fn cell_versions_reject_dangling_reverse_index() {
    let (store, id) = history_fixture();
    let mut batch = StoreBatch::new(&store);
    batch.put_spore_outpoint(&[0x61; 32], 0, &id);
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(
        &app,
        &format!(
            "/assets/identities/did/items/0x{}/cell-versions",
            hex::encode(id)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .contains("canonical marker"),
        "{json}"
    );
}

#[tokio::test]
async fn did_protocol_reads_original_local_cbor_and_rejects_missing_node() {
    use ckb_types::{
        bytes::Bytes,
        core::{BlockBuilder, EpochNumberWithFraction, TransactionBuilder},
        packed,
        prelude::*,
    };
    let id = vec![0x11; 20];
    let mut raw = 0u32.to_le_bytes().to_vec();
    // union0 + table(document Bytes, absent local_id), document = {alsoKnownAs:[at://alice.test]}.
    let cbor = hex::decode("a16b616c736f4b6e6f776e4173816f61743a2f2f616c6963652e74657374").unwrap();
    let total = 16 + cbor.len();
    raw.extend((total as u32).to_le_bytes());
    raw.extend(12u32.to_le_bytes());
    raw.extend((total as u32).to_le_bytes());
    raw.extend((cbor.len() as u32).to_le_bytes());
    raw.extend(&cbor);
    let mut payload = cell(&id, 320_00000000, 1);
    payload.data_size = raw.len() as i32;
    payload.occupied_capacity += (raw.len() as i64) * 100_000000;
    payload.data_hash = Some(ckb_hash::blake2b_256(&raw).to_vec());
    let lock = packed::Script::new_builder()
        .code_hash(packed::Byte32::from_slice(&payload.lock_code_hash).unwrap())
        .hash_type(1u8.into())
        .args(Bytes::from(payload.lock_args.clone()).pack())
        .build();
    let type_script = packed::Script::new_builder()
        .code_hash(packed::Byte32::from_slice(payload.type_code_hash.as_ref().unwrap()).unwrap())
        .hash_type(1u8.into())
        .args(Bytes::from(id.clone()).pack())
        .build();
    let output = packed::CellOutput::new_builder()
        .capacity((payload.capacity as u64).pack())
        .lock(lock)
        .type_(Some(type_script).pack())
        .build();
    let tx = TransactionBuilder::default()
        .output(output)
        .output_data(Bytes::from(raw.clone()).pack())
        .build();
    let hash: Vec<u8> = tx.hash().as_slice().to_vec();
    let chain = seed_ckb_chain(&[BlockBuilder::default()
        .number(100u64.pack())
        .epoch(EpochNumberWithFraction::new(1, 0, 1800).pack())
        .transaction(tx)
        .build()]);
    let store = test_store();
    let mut batch = StoreBatch::new(&store);
    batch.put_identity(
        &id,
        &IdentityEntry {
            standard: IdentityStandard::DidCkb,
            owner_lock_hash: Some(payload.lock_script_hash.clone()),
            name: None,
            is_live: true,
            created_at_block: 100,
            created_at_tx: hash.clone(),
            extra: IdentityExtra::DidCkb,
        },
    );
    batch.put_spore_outpoint(&hash, 0, &id);
    batch.put_cell(&hash, 0, &payload, 100);
    seed_tx(&mut batch, &hash, 100, 0);
    batch.commit().unwrap();
    let path = format!(
        "/assets/identities/did/items/0x{}/protocol",
        hex::encode(&id)
    );
    let app = create_router(test_config_with_ckb_db_path(
        store.clone(),
        store.clone(),
        chain.path,
        Some(chain.cleanup),
    ))
    .await;
    let (status, json) = get_json(&app, &path).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json["versions"][0]["dataHex"],
        format!("0x{}", hex::encode(raw))
    );
    assert_eq!(
        json["versions"][0]["decoded"]["document"]["alsoKnownAs"][0],
        "at://alice.test"
    );
    let mut config = test_config(store);
    config.ckb_db_path = "/nonexistent/ckbadger-asset-protocol-test".into();
    let app = create_router(config).await;
    let (status, json) = get_json(&app, &path).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .contains("local CKB RocksDB reader"),
        "{json}"
    );
}

fn mol_bytes(raw: &[u8]) -> Vec<u8> {
    let mut out = (raw.len() as u32).to_le_bytes().to_vec();
    out.extend(raw);
    out
}
fn mol_table(fields: &[Vec<u8>]) -> Vec<u8> {
    let head = 4 * (fields.len() + 1);
    let total = head + fields.iter().map(Vec::len).sum::<usize>();
    let mut out = (total as u32).to_le_bytes().to_vec();
    let mut offset = head;
    for f in fields {
        out.extend((offset as u32).to_le_bytes());
        offset += f.len()
    }
    for f in fields {
        out.extend(f)
    }
    out
}

async fn spore_genetics_fixture(pending: bool) -> axum::Router {
    spore_genetics_fixture_kind(pending, false).await
}
async fn spore_genetics_fixture_kind(pending: bool, ordinary: bool) -> axum::Router {
    spore_genetics_fixture_options(pending, if ordinary { 8 } else { 0 }, false, false).await
}
async fn spore_genetics_fixture_options(
    pending: bool,
    ordinary_count: u8,
    duplicate_traits: bool,
    extra_steps: bool,
) -> axum::Router {
    use ckb_types::{
        bytes::Bytes,
        core::{BlockBuilder, EpochNumberWithFraction, TransactionBuilder},
        packed,
        prelude::*,
    };
    let store = test_store();
    let cluster = vec![0x77; 32];
    let mut transactions = Vec::new();
    let mut owned = 0i128;
    let mut occupied = 0i128;
    let mut batch = StoreBatch::new(&store);
    batch.put_spore(&cluster,&ObjectEntry{standard:ObjectStandard::SporeCluster,collection_id:None,token_id:None,owner_lock_hash:None,name:Some("Genes".into()),description:Some(serde_json::json!({"dob":{"ver":0,"decoder":{"type":"code_hash","hash":"13cac78ad8482202f18f9df4ea707611c35f994375fa03ae79121312dda9925c"},"pattern":[["Face","String",0,1,"options",["Red","Blue"]]]}}).to_string()),is_live:false,created_at_block:99,created_at_tx:vec![0x78;32],extra:ObjectExtra::SporeCluster});
    for i in 0..8u8 {
        let ordinary = i < ordinary_count;
        let id = vec![i + 1; 32];
        let content =
            serde_json::json!({"id":u64::MAX-(i as u64),"dna":format!("{:02x}",i)}).to_string();
        let raw = mol_table(&[
            mol_bytes(if ordinary { b"text/plain" } else { b"dob/0" }),
            mol_bytes(content.as_bytes()),
            mol_bytes(&cluster),
        ]);
        let mut payload = cell(&id, 500_00000000, if i < 6 { 1 } else { 2 });
        payload.type_code_hash = Some(
            hex::decode(
                ckbadger_indexer::parser::spore::SPORE_CODE_HASH_MAINNET_V2
                    .trim_start_matches("0x"),
            )
            .unwrap(),
        );
        payload.type_script_hash = Some(compute_script_hash(
            payload.type_code_hash.as_ref().unwrap(),
            1,
            &id,
        ));
        payload.data_size = raw.len() as i32;
        payload.occupied_capacity = (126 + raw.len() as i64) * 100_000000;
        payload.data_hash = Some(ckb_hash::blake2b_256(&raw).to_vec());
        owned += i128::from(payload.capacity);
        occupied += i128::from(payload.occupied_capacity);
        let script = |code: &[u8], args: Vec<u8>| {
            packed::Script::new_builder()
                .code_hash(packed::Byte32::from_slice(code).unwrap())
                .hash_type(1u8.into())
                .args(Bytes::from(args).pack())
                .build()
        };
        let output = packed::CellOutput::new_builder()
            .capacity((payload.capacity as u64).pack())
            .lock(script(&payload.lock_code_hash, payload.lock_args.clone()))
            .type_(Some(script(payload.type_code_hash.as_ref().unwrap(), id.clone())).pack())
            .build();
        let tx = TransactionBuilder::default()
            .output(output)
            .output_data(Bytes::from(raw).pack())
            .build();
        let hash = tx.hash().as_slice().to_vec();
        batch.put_spore(
            &id,
            &ObjectEntry {
                standard: ObjectStandard::Spore,
                collection_id: Some(cluster.clone()),
                token_id: None,
                owner_lock_hash: Some(payload.lock_script_hash.clone()),
                name: None,
                description: None,
                is_live: true,
                created_at_block: 100,
                created_at_tx: hash.clone(),
                extra: ObjectExtra::Spore {
                    content_type: if ordinary { "text/plain" } else { "dob/0" }.into(),
                    content_length: content.len() as i64,
                    media_profile: SporeMediaProfile::default(),
                },
            },
        );
        batch.put_spore_by_cluster(&cluster, &id);
        batch.put_spore_outpoint(&hash, 0, &id);
        batch.put_cell(&hash, 0, &payload, 100);
        store
            .put_spore_daily_delta(
                &id,
                20231115,
                &SporeDailyDelta {
                    owned_capacity_delta: i128::from(payload.capacity),
                    owned_knowledge_delta: i128::from(payload.occupied_capacity),
                },
            )
            .unwrap();
        seed_tx(&mut batch, &hash, 100, i as i32);
        transactions.push(tx);
    }
    for (owner, count) in [(1, 6), (2, 2)] {
        let payload = cell(&[1; 32], 1000_00000000, owner);
        batch.put_cluster_owner_count(&cluster, &payload.lock_script_hash, count);
        batch.put_lock_script(
            &payload.lock_script_hash,
            &ckbadger_store::types::LockScriptEntry {
                code_hash: payload.lock_code_hash,
                hash_type: payload.lock_hash_type,
                args: payload.lock_args,
            },
        );
    }
    batch.put_cluster_aggregate(
        &cluster,
        &ClusterAggregate {
            total_count: 8,
            live_count: 8,
            owner_count: 2,
            owned_capacity: owned,
            owned_knowledge: occupied,
            unknown_count: 8,
            ..Default::default()
        },
    );
    batch.commit().unwrap();
    store
        .put_cluster_daily_delta(
            &cluster,
            20231115,
            &ClusterDailyDelta {
                owned_capacity_delta: owned,
                owned_knowledge_delta: occupied,
            },
        )
        .unwrap();
    if extra_steps {
        let mut entry = store.get_spore(&cluster).unwrap().unwrap();
        let mut metadata: serde_json::Value =
            serde_json::from_str(entry.description.as_ref().unwrap()).unwrap();
        let step = serde_json::json!({"decoder":metadata["dob"]["decoder"],"pattern":metadata["dob"]["pattern"]});
        metadata["dob"].as_object_mut().unwrap().remove("decoder");
        metadata["dob"].as_object_mut().unwrap().remove("pattern");
        metadata["dob"]["decoders"] = serde_json::json!([step, step]);
        entry.description = Some(metadata.to_string());
        store.put_spore_direct(&cluster, &entry).unwrap();
    }

    for i in 0..8u8 {
        if pending && i == 7 {
            continue;
        }
        store
            .put_dob_decoded_direct(
                &[i + 1; 32],
                &DobDecodedEntry {
                    steps: vec![ckbadger_store::DobDecodedStep {
                        step: 0,
                        media_type: "application/json".into(),
                        size: 1,
                        hash: "00".repeat(32),
                        traits: {
                            let mut traits = vec![DobDecodedTrait {
                                name: "Face".into(),
                                value: if i % 2 == 0 {
                                    "Red".into()
                                } else {
                                    "Blue".into()
                                },
                            }];
                            if duplicate_traits {
                                traits.push(traits[0].clone());
                                traits.push(DobDecodedTrait {
                                    name: "Face".into(),
                                    value: "Shared".into(),
                                });
                            }
                            traits
                        },
                    }],
                    media_sources: vec![],
                    decoded_at: 1_700_000_000,
                },
            )
            .unwrap();
    }
    // The collection's own Cell is separate from member holdings and their reverse index.
    let mut entry = store.get_spore(&cluster).unwrap().unwrap();
    let raw = mol_table(&[
        mol_bytes(entry.name.as_ref().unwrap().as_bytes()),
        mol_bytes(entry.description.as_ref().unwrap().as_bytes()),
        vec![],
    ]);
    let code = hex::decode(
        ckbadger_indexer::parser::spore::CLUSTER_CODE_HASH_MAINNET_V2.trim_start_matches("0x"),
    )
    .unwrap();
    let mut payload = cell(&cluster, 1000_00000000, 1);
    payload.type_code_hash = Some(code.clone());
    payload.type_script_hash = Some(compute_script_hash(&code, 1, &cluster));
    payload.data_size = raw.len() as i32;
    payload.occupied_capacity = (126 + raw.len() as i64) * 100_000000;
    payload.data_hash = Some(ckb_hash::blake2b_256(&raw).to_vec());
    let script = |code: &[u8], args: &[u8]| {
        packed::Script::new_builder()
            .code_hash(packed::Byte32::from_slice(code).unwrap())
            .hash_type(1u8.into())
            .args(Bytes::copy_from_slice(args).pack())
            .build()
    };
    let cluster_tx = TransactionBuilder::default()
        .output(
            packed::CellOutput::new_builder()
                .capacity((payload.capacity as u64).pack())
                .lock(script(&payload.lock_code_hash, &payload.lock_args))
                .type_(Some(script(&code, &cluster)).pack())
                .build(),
        )
        .output_data(Bytes::from(raw).pack())
        .build();
    let cluster_tx_hash = cluster_tx.hash().as_slice().to_vec();
    entry.created_at_tx = cluster_tx_hash.clone();
    entry.owner_lock_hash = Some(payload.lock_script_hash.clone());
    entry.is_live = true;
    let mut batch = StoreBatch::new(&store);
    batch.put_spore(&cluster, &entry);
    batch.put_cell(&cluster_tx_hash, 0, &payload, 99);
    batch.put_cell_by_type(
        payload.type_script_hash.as_ref().unwrap(),
        99,
        &cluster_tx_hash,
        0,
    );
    seed_tx(&mut batch, &cluster_tx_hash, 99, 0);
    batch.commit().unwrap();
    let chain = seed_ckb_chain(&[
        BlockBuilder::default()
            .number(99u64.pack())
            .epoch(EpochNumberWithFraction::new(1, 0, 1800).pack())
            .transaction(cluster_tx)
            .build(),
        BlockBuilder::default()
            .number(100u64.pack())
            .epoch(EpochNumberWithFraction::new(1, 0, 1800).pack())
            .transactions(transactions)
            .build(),
    ]);
    create_router(test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path,
        Some(chain.cleanup),
    ))
    .await
}

#[tokio::test]
async fn collection_insights_count_full_scope_and_gallery_filters_before_cursor() {
    let app = spore_genetics_fixture(false).await;
    let cluster = hex::encode([0x77; 32]);
    let (status, insights) = get_json(&app, &format!("/spore/clusters/0x{cluster}/insights")).await;
    assert_eq!(status, StatusCode::OK, "{insights}");
    capture_producer("collection-insights", &insights);
    let (status, gallery) = get_json(
        &app,
        &format!("/spore/clusters/0x{cluster}/gallery?limit=100"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{gallery}");
    assert_eq!(gallery["data"].as_array().unwrap().len(), 8);
    capture_producer("collection-gallery", &gallery);

    assert_eq!(insights["genetics"]["liveItems"], 8);
    assert_eq!(insights["genetics"]["decodedItems"], 8);
    assert_eq!(
        insights["genetics"]["traits"][0]["values"],
        serde_json::json!([{"value":"Blue","count":4},{"value":"Red","count":4}])
    );
    assert_eq!(insights["holderDistribution"][1]["items"], 2);
    assert_eq!(insights["holderDistribution"][3]["items"], 6);
    let filter = "%7B%22Face%22%3A%22Blue%22%7D";
    let url = format!("/spore/clusters/0x{cluster}/gallery?limit=1&traits={filter}");
    let (status, first) = get_json(&app, &url).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    capture_producer("collection-gallery-filtered", &first);
    assert_eq!(first["total"], 4);
    assert_eq!(first["data"][0]["dnaHex"], "0x01");
    assert_eq!(first["data"][0]["number"], (u64::MAX - 1).to_string());
    let (status, second) = get_json(
        &app,
        &format!("{url}&cursor={}", first["nextCursor"].as_str().unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["data"][0]["dnaHex"], "0x03");
    let (status, genetics) = get_json(
        &app,
        &format!("/spore/objects/0x{}/genetics", hex::encode([2; 32])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{genetics}");
    capture_producer("object-genetics", &genetics);
    assert_eq!(
        genetics["currentPattern"]["steps"][0]["bytes"][0]["rawHex"],
        "01"
    );
    assert_eq!(
        genetics["cachedVmTraits"]["originalDecoderHashRecorded"],
        false
    );
}

#[tokio::test]
async fn collection_pending_coverage_never_masquerades_as_complete_trait_frequency() {
    let app = spore_genetics_fixture(true).await;
    let cluster = hex::encode([0x77; 32]);
    let (status, json) = get_json(&app, &format!("/spore/clusters/0x{cluster}/insights")).await;
    capture_producer("collection-insights-pending", &json);
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["genetics"]["complete"], false);
    assert_eq!(json["genetics"]["pendingItems"], 1);
    assert_eq!(json["genetics"]["decodedItems"], 7);
    assert_eq!(json["genetics"]["traits"], serde_json::json!([]));
    let (status, json) = get_json(
        &app,
        &format!("/spore/clusters/0x{cluster}/gallery?traits=%7B%22Face%22%3A%22Blue%22%7D"),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{json}");
}

#[tokio::test]
async fn item_full_feeds_share_canonical_cursor_and_exact_item_capacity_context() {
    let (store, id) = history_fixture();
    seed_block_headers_at(&store, &[(100, 1_700_000_000_100)]);
    let header = store.get_block_header(100).unwrap().unwrap();
    let mut batch = StoreBatch::new(&store);
    for (index, hash, action) in [
        (1, [0x51; 32], AssetAction::Mint),
        (2, [0x52; 32], AssetAction::Transfer),
    ] {
        batch.put_tx_actions(&make_test_tx_actions(
            &cell(&id, 320_00000000, 2).lock_script_hash,
            &hash,
            &header.hash,
            100,
            index,
            100,
            0,
        ));
        batch
            .put_raw_cf_by_name(
                ckbadger_store::CF_STATS_SPORE,
                &ckbadger_store::keys::encode_spore_outpoint_by_id_key(&id, &hash, 0),
                &[action.compact()],
            )
            .unwrap();
    }
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let base = format!("/assets/identities/did/items/0x{}", hex::encode(&id));
    let (status, first) = get_json(&app, &format!("{base}/activity-feed?limit=1")).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    capture_producer("item-activity-feed", &first);
    assert_eq!(first["total"], 2);
    assert_eq!(first["nextCursor"], "100:2");
    assert_eq!(first["data"][0]["itemContext"]["action"], "transfer");
    assert_eq!(
        first["data"][0]["itemContext"]["createdCell"]["capacity"],
        "32000000000"
    );
    assert_eq!(
        first["data"][0]["itemContext"]["consumedCell"]["capacity"],
        "30700000000"
    );
    assert!(!first["data"][0]["participants"]
        .as_array()
        .unwrap()
        .is_empty());
    let (status, next) = get_json(&app, &format!("{base}/transactions?limit=1&cursor=100:2")).await;
    assert_eq!(status, StatusCode::OK, "{next}");
    capture_producer("item-transactions", &next);
    assert_eq!(next["total"], 2);
    assert_eq!(next["data"][0]["index"], 1);
    assert_eq!(next["data"][0]["itemContext"]["action"], "mint");
    assert!(next["data"][0]["itemContext"]["consumedCell"].is_null());
    let (status, error) = get_json(&app, &format!("{base}/activity-feed?cursor=100:3")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
}

async fn protocol_fixture(
    id: Vec<u8>,
    code: &str,
    raw: Vec<u8>,
    witnesses: Vec<Vec<u8>>,
    extra: Option<IdentityExtra>,
    lock_args: Option<Vec<u8>>,
) -> (axum::Router, Vec<u8>) {
    use ckb_types::{
        bytes::Bytes,
        core::{BlockBuilder, EpochNumberWithFraction, TransactionBuilder},
        packed,
        prelude::*,
    };
    let mut payload = cell(&id, 1000_00000000, 1);
    payload.type_code_hash = Some(hex::decode(code.trim_start_matches("0x")).unwrap());
    payload.type_script_hash = Some(compute_script_hash(
        payload.type_code_hash.as_ref().unwrap(),
        1,
        &id,
    ));
    if let Some(args) = lock_args {
        payload.lock_args = args;
        payload.lock_script_hash =
            compute_script_hash(&payload.lock_code_hash, 1, &payload.lock_args);
    }
    payload.data_size = raw.len() as i32;
    payload.occupied_capacity =
        ((8 + 33 + payload.lock_args.len() + 33 + id.len() + raw.len()) as i64) * 100_000000;
    payload.data_hash = Some(ckb_hash::blake2b_256(&raw).to_vec());
    let script = |code: &[u8], args: Vec<u8>| {
        packed::Script::new_builder()
            .code_hash(packed::Byte32::from_slice(code).unwrap())
            .hash_type(1u8.into())
            .args(Bytes::from(args).pack())
            .build()
    };
    let output = packed::CellOutput::new_builder()
        .capacity((payload.capacity as u64).pack())
        .lock(script(&payload.lock_code_hash, payload.lock_args.clone()))
        .type_(Some(script(payload.type_code_hash.as_ref().unwrap(), id.clone())).pack())
        .build();
    let tx = TransactionBuilder::default()
        .output(output)
        .output_data(Bytes::from(raw.clone()).pack())
        .witnesses(
            witnesses
                .into_iter()
                .map(|w| Bytes::from(w).pack())
                .collect::<Vec<_>>(),
        )
        .build();
    let hash = tx.hash().as_slice().to_vec();
    let chain = seed_ckb_chain(&[BlockBuilder::default()
        .number(100u64.pack())
        .epoch(EpochNumberWithFraction::new(1, 0, 1800).pack())
        .transaction(tx)
        .build()]);
    let store = test_store();
    let mut batch = StoreBatch::new(&store);
    if let Some(extra) = extra {
        let standard = match extra {
            IdentityExtra::DidCkb => IdentityStandard::DidCkb,
            IdentityExtra::BitCell { .. } => IdentityStandard::BitCell,
            IdentityExtra::DotBit { .. } => IdentityStandard::DotBit,
            IdentityExtra::DotCell { .. } => IdentityStandard::DotCell,
        };
        batch.put_identity_by_collection(
            ckbadger_store::types::identity_sentinel_for(standard),
            &id,
        );
        batch.put_identity(
            &id,
            &IdentityEntry {
                standard,
                owner_lock_hash: if standard == IdentityStandard::DotCell {
                    None
                } else {
                    Some(payload.lock_script_hash.clone())
                },
                name: None,
                is_live: true,
                created_at_block: 100,
                created_at_tx: hash.clone(),
                extra,
            },
        );
        if standard == IdentityStandard::DotBit {
            batch.put_dotbit_account_outpoint(&hash, 0, &id);
            batch
                .put_raw_cf_by_name(
                    ckbadger_store::CF_STATS_MNFT,
                    &ckbadger_store::keys::encode_dotbit_outpoint_by_account_id_key(&id, &hash, 0),
                    &[AssetAction::Mint.compact()],
                )
                .unwrap()
        } else {
            batch.put_spore_outpoint(&hash, 0, &id)
        }
    } else {
        batch.put_mnft(
            &id,
            &ObjectEntry {
                standard: ObjectStandard::MnftToken,
                collection_id: Some(id[..24].to_vec()),
                token_id: Some(1u32.to_be_bytes().to_vec()),
                owner_lock_hash: Some(payload.lock_script_hash.clone()),
                name: None,
                description: None,
                is_live: true,
                created_at_block: 100,
                created_at_tx: hash.clone(),
                extra: ObjectExtra::MnftToken {
                    token_index: 1,
                    characteristic: raw[1..9].to_vec(),
                    configure: raw[9],
                    state: raw[10],
                },
            },
        );
        batch.put_mnft_token_outpoint(&hash, 0, &id);
    }
    batch.put_cell(&hash, 0, &payload, 100);
    seed_tx(&mut batch, &hash, 100, 0);
    batch.commit().unwrap();
    (
        create_router(test_config_with_ckb_db_path(
            store.clone(),
            store,
            chain.path,
            Some(chain.cleanup),
        ))
        .await,
        hash,
    )
}

#[tokio::test]
async fn mnft_protocol_exposes_exact_configure_state_and_characteristic() {
    let id = vec![0x12; 28];
    let mut raw = vec![0];
    raw.extend([0x98; 8]);
    raw.extend([0xc0, 3]);
    let (app, _) = protocol_fixture(
        id.clone(),
        ckbadger_indexer::parser::mnft::MNFT_TOKEN_CODE_HASH,
        raw,
        vec![],
        None,
        None,
    )
    .await;
    let (status, json) = get_json(
        &app,
        &format!("/assets/objects/items/0x{}/protocol", hex::encode(id)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    capture_producer("mnft-protocol", &json);
    let decoded = &json["versions"][0]["decoded"];
    assert_eq!(
        decoded["characteristicHex"],
        format!("0x{}", hex::encode([0x98; 8]))
    );
    assert_eq!(decoded["configure"], 192);
    assert_eq!(decoded["claimed"], true);
    assert_eq!(decoded["locked"], true);
    assert_eq!(decoded["permissions"]["allowTransferAfterClaim"], true);
    assert_eq!(decoded["permissions"]["allowDestroyAfterClaim"], false);
}

#[tokio::test]
async fn bit_cell_protocol_decodes_both_chain_layouts_without_merging_account_identity() {
    let account = b"alice.bit";
    let account_id = ckb_hash::blake2b_256(account)[..20].to_vec();
    for legacy in [true, false] {
        let id = vec![if legacy { 0x21 } else { 0x22 }; 32];
        let raw = if legacy {
            let mut raw = 0u32.to_le_bytes().to_vec();
            raw.extend(mol_table(&[
                vec![0x44; 20],
                1_900_000_000u64.to_le_bytes().to_vec(),
                mol_bytes(account),
            ]));
            raw
        } else {
            let mut content = vec![0, 1];
            content.extend([0x44; 20]);
            content.extend(1_900_000_000u64.to_le_bytes());
            content.extend(account);
            mol_table(&[mol_bytes(b"application/did"), mol_bytes(&content), vec![]])
        };
        let (app, _) = protocol_fixture(
            id.clone(),
            ckbadger_indexer::parser::bit_cell::BIT_CELL_CODE_HASH_MAINNET,
            raw,
            vec![],
            Some(IdentityExtra::BitCell {
                account_id: account_id.clone(),
                expired_at: 1_900_000_000,
            }),
            None,
        )
        .await;
        let (status, json) = get_json(
            &app,
            &format!(
                "/assets/identities/bit-cell/items/0x{}/protocol",
                hex::encode(&id)
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "legacy={legacy}: {json}");
        capture_producer(
            if legacy {
                "bit-cell-legacy-protocol"
            } else {
                "bit-cell-protocol"
            },
            &json,
        );
        let d = &json["versions"][0]["decoded"];
        assert_eq!(d["identityId"], format!("0x{}", hex::encode(id)));
        assert_eq!(d["accountId"], format!("0x{}", hex::encode(&account_id)));
        assert_eq!(d["account"], "alice.bit");
        assert_eq!(d["expiredAt"], "1900000000");
        assert!(d["linkedAccount"].is_null());
    }
}

fn dotbit_witness(version: u32, id: &[u8], index: u32) -> (Vec<u8>, Vec<u8>) {
    let record = mol_table(&[
        mol_bytes(b"address"),
        mol_bytes(b"60"),
        mol_bytes(b"main"),
        mol_bytes(b"0xabc"),
        300u32.to_le_bytes().to_vec(),
    ]);
    let chars = mol_table(&[mol_table(&[
        0u32.to_le_bytes().to_vec(),
        mol_bytes(b"alice"),
    ])]);
    let mut fields = vec![id.to_vec(), chars, 1_600_000_000u64.to_le_bytes().to_vec()];
    if version == 1 {
        fields.push(1_700_000_000u64.to_le_bytes().to_vec())
    } else {
        fields.extend([
            0u64.to_le_bytes().to_vec(),
            0u64.to_le_bytes().to_vec(),
            0u64.to_le_bytes().to_vec(),
        ])
    }
    fields.extend([vec![0], mol_table(&[record])]);
    if version >= 3 {
        fields.extend([vec![0], 0u64.to_le_bytes().to_vec()]);
    }
    if version >= 4 {
        fields.push(mol_table(&[mol_bytes(b""), mol_bytes(b"")]))
    }
    let entity = mol_table(&fields);
    let new = mol_table(&[
        index.to_le_bytes().to_vec(),
        version.to_le_bytes().to_vec(),
        mol_bytes(&entity),
    ]);
    let mut witness = b"das\x01\0\0\0".to_vec();
    witness.extend(mol_table(&[vec![], vec![], new]));
    (entity, witness)
}
#[tokio::test]
async fn dotbit_protocol_reads_committed_records_and_parties_for_each_witness_version() {
    for version in 1..=4 {
        let id = vec![0x33; 20];
        let (entity, witness) = dotbit_witness(version, &id, 0);
        let mut raw = ckb_hash::blake2b_256(&entity).to_vec();
        raw.extend(&id);
        raw.extend([0x55; 20]);
        raw.extend(1_900_000_000u64.to_le_bytes());
        let mut lock = vec![3];
        lock.extend([0x66; 20]);
        lock.push(6);
        lock.extend([0x77; 32]);
        let (app, _) = protocol_fixture(
            id.clone(),
            ckbadger_indexer::parser::dotbit::DOTBIT_ACCOUNT_CELL_TYPE_ID,
            raw,
            vec![witness],
            Some(IdentityExtra::DotBit {
                expired_at: Some(1_900_000_000),
                registered_at: Some(1_600_000_000),
                status: Some(0),
            }),
            Some(lock),
        )
        .await;
        let (status, json) = get_json(
            &app,
            &format!(
                "/assets/identities/dotbit/items/0x{}/protocol",
                hex::encode(id)
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "version={version}: {json}");
        capture_producer(&format!("dotbit-v{version}-protocol"), &json);
        let d = &json["versions"][0]["decoded"];
        assert_eq!(d["witnessVersion"], version);
        assert_eq!(d["account"], "alice.bit");
        assert_eq!(d["registeredAt"], "1600000000");
        assert_eq!(d["records"][0]["ttl"], 300);
        assert_eq!(d["records"][0]["value"], "0xabc");
        assert_eq!(d["owner"]["algorithm"], 3);
        assert_eq!(d["manager"]["algorithm"], 6);
    }
}

#[tokio::test]
async fn dotcell_protocol_keeps_named_owner_prefix_separate_from_physical_lock() {
    use ckbadger_indexer::parser::DotCellParser;
    let id = DotCellParser::derive_id("alice").to_vec();
    let records = vec![0u8, 0];
    let records_hash = DotCellParser::records_hash(&records);
    let mut raw = vec![3];
    raw.extend(records_hash);
    raw.extend([0; 20]);
    raw.extend(&1_900_000_000u64.to_le_bytes()[..5]);
    raw.extend([0x61; 20]);
    raw.extend([0x62; 20]);
    raw.extend(b"alice");
    let witness = mol_table(&[vec![], vec![], mol_bytes(&records)]);
    let extra = IdentityExtra::DotCell {
        label: "alice".into(),
        namespace_args: id.as_slice().try_into().unwrap(),
        layout_version: 3,
        expired_at: 1_900_000_000,
        owner_hash20: [0x61; 20],
        manager_hash20: [0x62; 20],
        next_id: [0; 20],
        records_hash,
        records: vec![],
        parent_id: None,
    };
    let (app, _) = protocol_fixture(
        id.clone(),
        &hex::encode([0x41; 32]),
        raw,
        vec![witness],
        Some(extra),
        None,
    )
    .await;
    let (status, json) =
        get_json(&app, "/assets/identities/dotcell/items/alice.cell/protocol").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    capture_producer("dotcell-protocol", &json);
    let v = &json["versions"][0];
    assert_eq!(v["decoded"]["name"], "alice.cell");
    assert_eq!(
        v["decoded"]["owner"]["hashPrefix"],
        format!("0x{}", hex::encode([0x61; 20]))
    );
    assert_ne!(
        v["decoded"]["owner"]["hashPrefix"],
        v["cell"]["ownerLockHash"]
    );
    assert_eq!(v["decoded"]["records"], serde_json::json!([]));
}

#[tokio::test]
async fn collection_holders_resolve_real_lock_addresses() {
    let app = spore_genetics_fixture(false).await;
    let (status, json) = get_json(
        &app,
        &format!("/spore/clusters/0x{}/holders", hex::encode([0x77; 32])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["total"], 2);
    assert_eq!(json["data"][0]["itemCount"], 6);
    assert!(
        json["data"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["address"]
                .as_str()
                .is_some_and(|s| s.starts_with("ckb1"))),
        "{json}"
    );
}

#[tokio::test]
async fn did_pds_groups_count_each_live_identity_once_per_endpoint() {
    let document = serde_json::json!({"alsoKnownAs":["at://alice.test"],"serial":u64::MAX,"services":{"atproto_pds":{"type":"AtprotoPersonalDataServer","endpoint":"https://pds.example"}},"service":[{"type":"AtprotoPersonalDataServer","serviceEndpoint":"https://pds.example"}]});
    let mut cbor = Vec::new();
    ciborium::into_writer(&document, &mut cbor).unwrap();
    let mut raw = 0u32.to_le_bytes().to_vec();
    raw.extend(mol_table(&[mol_bytes(&cbor), mol_bytes(b"local-1")]));
    let id = vec![0x11; 32];
    let (app, _) = protocol_fixture(
        id.clone(),
        &hex::encode([0x42; 32]),
        raw,
        vec![],
        Some(IdentityExtra::DidCkb),
        None,
    )
    .await;
    let (status, json) = get_json(&app, "/assets/identities/did/pds").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    capture_producer("did-pds", &json);
    assert_eq!(json["totalLiveIdentities"], 1);
    assert_eq!(json["withoutPds"], 0);
    assert_eq!(
        json["groups"],
        serde_json::json!([{"endpoint":"https://pds.example","count":1}])
    );
    let (status, protocol) = get_json(
        &app,
        &format!(
            "/assets/identities/did/items/0x{}/protocol",
            hex::encode(id)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{protocol}");
    capture_producer("did-protocol", &protocol);
    assert_eq!(
        protocol["versions"][0]["decoded"]["document"]["serial"],
        serde_json::json!({"$integer":u64::MAX.to_string()})
    );
    assert_eq!(protocol["versions"][0]["decoded"]["localId"], "local-1");
}

#[tokio::test]
async fn collection_full_feeds_exclude_orphan_activity_rows_and_share_pagination() {
    let (store, _) = history_fixture();
    seed_block_headers_at(&store, &[(100, 1_700_000_000_100)]);
    let header = store.get_block_header(100).unwrap().unwrap();
    let collection = ckbadger_store::types::identity_sentinel_for(IdentityStandard::DidCkb);
    let mut batch = StoreBatch::new(&store);
    batch.put_identity_collection_aggregate(
        collection,
        &IdentityCollectionAggregate {
            standard: IdentityStandard::DidCkb,
            total_count: 1,
            live_count: 1,
            holders_count: 1,
            activities_count: 2,
            ..Default::default()
        },
    );
    for (index, hash, action) in [
        (1, [0x51; 32], AssetAction::Mint),
        (2, [0x52; 32], AssetAction::Transfer),
    ] {
        batch.put_tx_actions(&make_test_tx_actions(
            &[0x11; 32],
            &hash,
            &header.hash,
            100,
            index,
            100,
            0,
        ));
        batch.put_identity_collection_activity(
            collection,
            100,
            index,
            &ObjectCollectionActivityEntry {
                tx_hash: hash.to_vec(),
                block_hash: header.hash.clone(),
                timestamp_ms: 1_700_000_000_100,
                actions: vec![action],
            },
        );
    }
    batch.put_identity_collection_activity(
        collection,
        101,
        0,
        &ObjectCollectionActivityEntry {
            tx_hash: vec![0xfe; 32],
            block_hash: vec![0xff; 32],
            timestamp_ms: 1_700_000_000_101,
            actions: vec![AssetAction::Mint],
        },
    );
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let (status, activities) =
        get_json(&app, "/assets/identities/did:ckb/activity-feed?limit=1").await;
    assert_eq!(status, StatusCode::OK, "{activities}");
    assert_eq!(activities["total"], 2);
    assert_eq!(activities["nextCursor"], "100:2");
    let (status, transactions) = get_json(
        &app,
        "/assets/identities/did:ckb/transactions?cursor=100:2&limit=1",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{transactions}");
    assert_eq!(transactions["total"], 2);
    assert_eq!(transactions["data"][0]["index"], 1);
}

#[tokio::test]
async fn ordinary_spores_are_not_pending_dob_decodes_or_trait_filter_members() {
    let app = spore_genetics_fixture_kind(true, true).await;
    let base = format!("/spore/clusters/0x{}", hex::encode([0x77; 32]));
    let (status, json) = get_json(&app, &format!("{base}/insights")).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["genetics"]["liveItems"], 8);
    assert_eq!(json["genetics"]["eligibleItems"], 0);
    assert_eq!(json["genetics"]["nonDobItems"], 8);
    assert_eq!(json["genetics"]["pendingItems"], 0);
    let (status, json) = get_json(
        &app,
        &format!("{base}/gallery?traits=%7B%22Face%22%3A%22Red%22%7D"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["total"], 0);
    let (status, json) = get_json(
        &app,
        &format!("/spore/objects/0x{}/genetics", hex::encode([1; 32])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert!(json["dnaHex"].is_null());
    assert_eq!(json["cachedVmTraits"]["status"], "notDob");
}

#[tokio::test]
async fn item_history_rejects_reverse_forward_identity_mismatch() {
    let (store, id) = history_fixture();
    let mut batch = StoreBatch::new(&store);
    batch.put_spore_outpoint(&[0x52; 32], 0, &[0x99; 20]);
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(
        &app,
        &format!(
            "/assets/identities/did/items/0x{}/cell-versions",
            hex::encode(&id)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    assert!(
        json["message"].as_str().unwrap().contains("forward"),
        "{json}"
    );
    let (status, json) = get_json(
        &app,
        &format!("/assets/identities/did/items/0x{}", hex::encode(&id)),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    assert!(
        json["message"].as_str().unwrap().contains("forward"),
        "{json}"
    );
}
#[tokio::test]
async fn burned_item_cannot_report_empty_canonical_history() {
    let store = test_store();
    let id = [0x44; 20];
    let mut batch = StoreBatch::new(&store);
    batch.put_identity(
        &id,
        &IdentityEntry {
            standard: IdentityStandard::DidCkb,
            owner_lock_hash: None,
            name: None,
            is_live: false,
            created_at_block: 100,
            created_at_tx: vec![0x51; 32],
            extra: IdentityExtra::DidCkb,
        },
    );
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(
        &app,
        &format!(
            "/assets/identities/did/items/0x{}/cell-versions",
            hex::encode(id)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    assert!(
        json["message"].as_str().unwrap().contains("history"),
        "{json}"
    );
}
#[tokio::test]
async fn item_history_rejects_current_owner_metadata_disagreement() {
    let (store, id) = history_fixture();
    let mut entry = store.get_identity(&id).unwrap().unwrap();
    entry.owner_lock_hash = Some(vec![0xaa; 32]);
    let mut batch = StoreBatch::new(&store);
    batch.put_identity(&id, &entry);
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(
        &app,
        &format!(
            "/assets/identities/did/items/0x{}/cell-versions",
            hex::encode(id)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    assert!(
        json["message"].as_str().unwrap().contains("owner"),
        "{json}"
    );
}
#[tokio::test]
async fn weekly_collection_history_uses_complete_indexed_calendar_and_quiet_zero_weeks() {
    let store = test_store();
    let cluster = [0x77; 32];
    seed_block_headers_at(
        &store,
        &[
            (0, utc_ms("2026-10-12T00:00:00Z")),
            (1, utc_ms("2026-10-18T15:59:00Z")),
            (2, utc_ms("2026-10-19T00:00:00Z")),
            (3, utc_ms("2026-10-26T00:00:00Z")),
            (4, utc_ms("2026-10-18T00:00:00Z")),
        ],
    );
    let mut batch = StoreBatch::new(&store);
    batch.put_spore(
        &cluster,
        &ObjectEntry {
            standard: ObjectStandard::SporeCluster,
            collection_id: None,
            token_id: None,
            owner_lock_hash: None,
            name: Some("Calendar".into()),
            description: None,
            is_live: false,
            created_at_block: 0,
            created_at_tx: vec![0x11; 32],
            extra: ObjectExtra::SporeCluster,
        },
    );
    batch.put_cluster_aggregate(&cluster, &ClusterAggregate::default());
    for (block, index, hash, action) in [
        (0, 1, [0x11; 32], AssetAction::Mint),
        (1, 1, [0x12; 32], AssetAction::Transfer),
        (4, 1, [0x13; 32], AssetAction::Transfer),
    ] {
        seed_tx(&mut batch, &hash, block, index);
        let header = store.get_block_header(block).unwrap().unwrap();
        batch.put_object_collection_activity(
            &cluster,
            block,
            index,
            &ObjectCollectionActivityEntry {
                tx_hash: hash.to_vec(),
                block_hash: header.hash,
                timestamp_ms: header.timestamp,
                actions: vec![action],
            },
        );
    }
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(
        &app,
        &format!("/spore/clusters/0x{}/insights", hex::encode(cluster)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["completeThrough"], "2026-10-25");
    assert_eq!(json["calendarAnchor"]["blockNumber"], 4);
    assert_eq!(
        json["weeklyActivity"],
        serde_json::json!([{"weekStart":"2026-10-12","mintTransactions":1,"transferTransactions":1,"burnTransactions":0},{"weekStart":"2026-10-19","mintTransactions":0,"transferTransactions":0,"burnTransactions":0}])
    );
    capture_producer("collection-complete-week-history", &json);
}

#[tokio::test]
async fn history_consumer_must_follow_creation_even_in_same_block() {
    let (store, id) = history_fixture();
    let mut batch = StoreBatch::new(&store);
    batch.put_consumed_cell_with_consumer(
        &[0x51; 32],
        0,
        &cell(&id, 307_00000000, 1),
        100,
        100,
        Some(&[0x51; 32]),
    );
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(
        &app,
        &format!(
            "/assets/identities/did/items/0x{}/cell-versions",
            hex::encode(id)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    assert!(
        json["message"].as_str().unwrap().contains("consumer order"),
        "{json}"
    );
}

#[tokio::test]
async fn empty_cluster_feeds_exist_and_spore_namespace_rejects_identity_sentinel() {
    let store = test_store();
    let id = [0xca; 32];
    let mut batch = StoreBatch::new(&store);
    batch.put_spore(
        &id,
        &ObjectEntry {
            standard: ObjectStandard::SporeCluster,
            collection_id: None,
            token_id: None,
            owner_lock_hash: None,
            name: Some("Empty".into()),
            description: None,
            is_live: true,
            created_at_block: 10,
            created_at_tx: vec![0xaa; 32],
            extra: ObjectExtra::SporeCluster,
        },
    );
    let identity = ckbadger_store::types::identity_sentinel_for(IdentityStandard::DidCkb);
    batch.put_identity_collection_aggregate(
        identity,
        &IdentityCollectionAggregate {
            standard: IdentityStandard::DidCkb,
            total_count: 1,
            live_count: 1,
            ..Default::default()
        },
    );
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    for id in [format!("0x{}", hex::encode(id)), "sole-spores".into()] {
        for route in ["activity-feed", "transactions"] {
            let (status, json) = get_json(&app, &format!("/spore/clusters/{id}/{route}")).await;
            assert_eq!(status, StatusCode::OK, "{json}");
            assert_eq!(json["total"], 0);
            assert_eq!(json["data"], serde_json::json!([]));
        }
    }
    for route in ["activity-feed", "transactions"] {
        let (status, json) = get_json(
            &app,
            &format!("/spore/clusters/0x{}/{route}", hex::encode(identity)),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{json}");
    }
}

#[tokio::test]
async fn mixed_collection_coverage_and_multivalue_frequency_are_per_item() {
    let app = spore_genetics_fixture_options(false, 2, true, false).await;
    let (status, json) = get_json(
        &app,
        &format!("/spore/clusters/0x{}/insights", hex::encode([0x77; 32])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["genetics"]["nonDobItems"], 2);
    assert_eq!(json["genetics"]["eligibleItems"], 6);
    assert_eq!(json["genetics"]["decodedItems"], 6);
    assert_eq!(
        json["genetics"]["traits"][0]["values"],
        serde_json::json!([{"value":"Blue","count":3},{"value":"Red","count":3},{"value":"Shared","count":6}])
    );
}
#[tokio::test]
async fn dob0_current_pattern_uses_only_worker_first_step_dispatch() {
    let app = spore_genetics_fixture_options(false, 0, false, true).await;
    let (status, json) = get_json(
        &app,
        &format!("/spore/objects/0x{}/genetics", hex::encode([1; 32])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json["currentPattern"]["steps"].as_array().unwrap().len(),
        1,
        "{json}"
    );
}

#[tokio::test]
async fn same_spore_producer_detail_protocol_genetics_and_cell_versions_agree() {
    let app = spore_genetics_fixture(false).await;
    let item = format!("/spore/objects/0x{}", hex::encode([2; 32]));
    let (status, detail) = get_json(&app, &item).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    capture_producer("spore-detail", &detail);
    let (status, protocol) = get_json(&app, &format!("{item}/protocol")).await;
    assert_eq!(status, StatusCode::OK, "{protocol}");
    capture_producer("spore-protocol", &protocol);
    assert_eq!(protocol["versions"][0]["decoded"]["dnaHex"], "0x01");
    let (status, versions) = get_json(&app, &format!("{item}/cell-versions")).await;
    assert_eq!(status, StatusCode::OK, "{versions}");
    capture_producer("spore-cell-versions", &versions);
    assert_eq!(protocol["versions"][0]["cell"], versions["data"][0]);
    assert_eq!(detail["ownedCapacity"], versions["data"][0]["capacity"]);
    let (status, cluster) = get_json(
        &app,
        &format!("/spore/clusters/0x{}", hex::encode([0x77; 32])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{cluster}");
    capture_producer("cluster-detail", &cluster);
    assert_eq!(cluster["ownedCapacity"], (500_00000000i128 * 8).to_string());
    assert_eq!(cluster["composition"]["unknownCount"], 8);
    let (status, cluster_protocol) = get_json(
        &app,
        &format!("/spore/clusters/0x{}/protocol", hex::encode([0x77; 32])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{cluster_protocol}");
    capture_producer("cluster-protocol", &cluster_protocol);
    assert_eq!(
        cluster_protocol["current"]["decoded"]["description"],
        cluster["description"]
    );
    assert_eq!(
        cluster_protocol["current"]["cell"]["ownerLockHash"],
        cluster["ownerLockHash"]
    );
    assert_eq!(
        cluster_protocol["current"]["cell"]["blockNumber"],
        cluster["createdAtBlock"]
    );
    let (status, collection) = get_json(
        &app,
        &format!("/assets/objects/0x{}", hex::encode([0x77; 32])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{collection}");
    capture_producer("object-collection-detail", &collection);
    assert_eq!(collection["ownedCapacity"], cluster["ownedCapacity"]);
    assert_eq!(collection["activitiesCount"], cluster["activitiesCount"]);
}

#[tokio::test]
async fn capacity_charts_exclude_incomplete_day_using_indexed_chain_clock() {
    let store = test_store();
    seed_block_headers_at(
        &store,
        &[
            (0, utc_ms("2026-10-25T00:00:00Z")),
            (1, utc_ms("2026-10-27T00:00:00Z")),
            (2, utc_ms("2026-10-24T00:00:00Z")),
        ],
    );
    let cluster = [0x77; 32];
    let item = [0x78; 32];
    let mut batch = StoreBatch::new(&store);
    batch.put_spore(
        &cluster,
        &ObjectEntry {
            standard: ObjectStandard::SporeCluster,
            collection_id: None,
            token_id: None,
            owner_lock_hash: None,
            name: Some("Calendar".into()),
            description: None,
            is_live: false,
            created_at_block: 0,
            created_at_tx: vec![0x11; 32],
            extra: ObjectExtra::SporeCluster,
        },
    );
    batch.put_spore(
        &item,
        &ObjectEntry {
            standard: ObjectStandard::Spore,
            collection_id: Some(cluster.to_vec()),
            token_id: None,
            owner_lock_hash: Some(vec![0x55; 32]),
            name: None,
            description: None,
            is_live: true,
            created_at_block: 0,
            created_at_tx: vec![0x12; 32],
            extra: ObjectExtra::Spore {
                content_type: "text/plain".into(),
                content_length: 0,
                media_profile: SporeMediaProfile::default(),
            },
        },
    );
    batch.put_cluster_aggregate(
        &cluster,
        &ClusterAggregate {
            total_count: 3,
            live_count: 1,
            owner_count: 1,
            unknown_count: 1,
            ..Default::default()
        },
    );
    batch.commit().unwrap();
    for (date, capacity, used) in [(20261025, 100, 50), (20261026, 20, 10), (20261027, 999, 99)] {
        store
            .put_spore_daily_delta(
                &item,
                date,
                &SporeDailyDelta {
                    owned_capacity_delta: capacity,
                    owned_knowledge_delta: used,
                },
            )
            .unwrap();
        store
            .put_cluster_daily_delta(
                &cluster,
                date,
                &ClusterDailyDelta {
                    owned_capacity_delta: capacity,
                    owned_knowledge_delta: used,
                },
            )
            .unwrap();
    }
    let app = create_router(test_config(store)).await;
    let (status, detail) =
        get_json(&app, &format!("/spore/clusters/0x{}", hex::encode(cluster))).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    // total/live distinction is asserted after the calendar checks below.
    assert_eq!(detail["sporesCount"], 1);
    for (name, base) in [
        (
            "cluster-capacity-history",
            format!("/spore/clusters/0x{}", hex::encode(cluster)),
        ),
        (
            "spore-capacity-history",
            format!("/spore/objects/0x{}", hex::encode(item)),
        ),
        (
            "object-collection-capacity-history",
            format!("/assets/objects/0x{}", hex::encode(cluster)),
        ),
    ] {
        let (status, json) = get_json(
            &app,
            &format!("{base}/charts/capacity-history?to=2026-10-29"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["data"].as_array().unwrap().len(), 2, "{json}");
        assert_eq!(json["data"][1]["date"], "2026-10-26");
        assert_eq!(
            json["data"][1]["values"],
            serde_json::json!({"used":"60","unused":"60"})
        );
        assert_eq!(json["completeThrough"], "2026-10-26");
        assert_eq!(json["anchor"]["blockNumber"], 2);
        assert_eq!(json["source"], "indexedChainTimeUtc8");
        capture_producer(name, &json);
        let (status, json) = get_json(
            &app,
            &format!("{base}/charts/capacity-history?from=2026-10-27&to=2026-10-29"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["data"], serde_json::json!([]));
    }
    assert_eq!(detail["totalCount"], 3);
}

async fn cluster_protocol_fixture(mode: u8) -> axum::Router {
    use ckb_types::{
        bytes::Bytes,
        core::{BlockBuilder, EpochNumberWithFraction, TransactionBuilder},
        packed,
        prelude::*,
    };
    let store = test_store();
    let id = [0x77; 32];
    let description=serde_json::json!({"dob":{"ver":0,"decoder":{"type":"code_hash","hash":"13cac78ad8482202f18f9df4ea707611c35f994375fa03ae79121312dda9925c"},"pattern":[["Face","String",0,1,"options",["Red","Blue"]]]}}).to_string();
    let mut fields = vec![mol_bytes(b"Genes"), mol_bytes(description.as_bytes())];
    if mode != 4 {
        fields.push(vec![]);
    }
    let raw = mol_table(&fields);
    let code = hex::decode(
        if mode == 4 {
            ckbadger_indexer::parser::spore::CLUSTER_CODE_HASH_TESTNET_V1
        } else {
            ckbadger_indexer::parser::spore::CLUSTER_CODE_HASH_MAINNET_V2
        }
        .trim_start_matches("0x"),
    )
    .unwrap();
    let mut payload = cell(&id, 1000_00000000, 1);
    payload.type_code_hash = Some(code.clone());
    payload.type_script_hash = Some(compute_script_hash(&code, 1, &id));
    payload.data_size = raw.len() as i32;
    payload.occupied_capacity = (126 + raw.len() as i64) * 100_000000;
    payload.data_hash = Some(ckb_hash::blake2b_256(&raw).to_vec());
    let script = |code: &[u8], args: &[u8]| {
        packed::Script::new_builder()
            .code_hash(packed::Byte32::from_slice(code).unwrap())
            .hash_type(1u8.into())
            .args(Bytes::copy_from_slice(args).pack())
            .build()
    };
    let tx = TransactionBuilder::default()
        .output(
            packed::CellOutput::new_builder()
                .capacity((payload.capacity as u64).pack())
                .lock(script(&payload.lock_code_hash, &payload.lock_args))
                .type_(Some(script(&code, &id)).pack())
                .build(),
        )
        .output_data(Bytes::from(raw).pack())
        .build();
    let hash = tx.hash().as_slice().to_vec();
    let chain = seed_ckb_chain(&[BlockBuilder::default()
        .number(100u64.pack())
        .epoch(EpochNumberWithFraction::new(1, 0, 1800).pack())
        .transaction(tx)
        .build()]);
    let mut batch = StoreBatch::new(&store);
    batch.put_spore(
        &id,
        &ObjectEntry {
            standard: ObjectStandard::SporeCluster,
            collection_id: None,
            token_id: None,
            owner_lock_hash: Some(payload.lock_script_hash.clone()),
            name: Some("Genes".into()),
            description: Some(description.clone()),
            is_live: true,
            created_at_block: 100,
            created_at_tx: hash.clone(),
            extra: ObjectExtra::SporeCluster,
        },
    );
    // Live and bulk writers create the aggregate with the cluster entry.
    batch.put_cluster_aggregate(
        &id,
        &ClusterAggregate {
            name: Some("Genes".into()),
            description: Some(description),
            ..Default::default()
        },
    );
    if mode > 0 {
        batch.put_cell(&hash, 0, &payload, 100);
        batch.put_cell_by_type(
            payload.type_script_hash.as_ref().unwrap(),
            if mode == 2 { 99 } else { 100 },
            &hash,
            0,
        );
        seed_tx(&mut batch, &hash, 100, 1);
    }
    if mode == 3 {
        batch.put_cell(&[0xff; 32], 0, &payload, 100);
        batch.put_cell_by_type(
            payload.type_script_hash.as_ref().unwrap(),
            100,
            &[0xff; 32],
            0,
        );
        seed_tx(&mut batch, &[0xff; 32], 100, 2);
    }
    batch.commit().unwrap();
    create_router(test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path,
        Some(chain.cleanup),
    ))
    .await
}
#[tokio::test]
async fn cluster_protocol_reads_current_type_index_and_never_item_reverse_history() {
    let app = cluster_protocol_fixture(1).await;
    let url = format!("/spore/clusters/0x{}", hex::encode([0x77; 32]));
    let (status, json) = get_json(&app, &format!("{url}/protocol")).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["current"]["decoded"]["name"], "Genes");
    assert!(json["current"]["decoded"]["description"]
        .as_str()
        .unwrap()
        .contains("dob"));
    assert_eq!(json["current"]["cell"]["capacity"], "100000000000");
    assert_eq!(json["source"], "canonicalLiveTypeIndex");
    let (status, detail) = get_json(&app, &url).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(
        detail["ownerLockHash"],
        json["current"]["cell"]["ownerLockHash"]
    );
    assert_eq!(
        detail["ownerAddress"],
        json["current"]["cell"]["ownerAddress"]
    );
}
#[tokio::test]
async fn cluster_metadata_does_not_imply_a_current_cell_or_owner() {
    let app = cluster_protocol_fixture(0).await;
    let url = format!("/spore/clusters/0x{}", hex::encode([0x77; 32]));
    let (status, json) = get_json(&app, &format!("{url}/protocol")).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert!(json["current"].is_null());
    let (status, detail) = get_json(&app, &url).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert!(detail["ownerLockHash"].is_null(), "{detail}");
    assert!(detail["ownerAddress"].is_null());
}
#[tokio::test]
async fn cluster_current_cell_rejects_stale_type_key_and_multiple_live_cells() {
    for mode in [2, 3] {
        let app = cluster_protocol_fixture(mode).await;
        let (status, json) = get_json(
            &app,
            &format!("/spore/clusters/0x{}/protocol", hex::encode([0x77; 32])),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
        assert!(
            json["message"].as_str().unwrap().contains(if mode == 2 {
                "disagrees"
            } else {
                "multiple live"
            }),
            "{json}"
        );
    }
}

#[tokio::test]
async fn canonical_versions_reject_payload_script_hash_mismatch() {
    for kind in ["lock", "type"] {
        let (store, id) = history_fixture_with_corrupt_script(Some(kind));
        let app = create_router(test_config(store)).await;
        let (status, json) = get_json(
            &app,
            &format!(
                "/assets/identities/did/items/0x{}/cell-versions",
                hex::encode(id)
            ),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{kind}: {json}");
        let message = json["message"].as_str().unwrap();
        assert!(
            message.contains(&format!("{kind} script hash mismatch")),
            "{json}"
        );
        assert!(
            message.contains(&format!("outpoint=0x{}:0", hex::encode([0x52; 32]))),
            "{json}"
        );
    }
}

#[tokio::test]
async fn cluster_capacity_chart_rejects_spore_item_namespace() {
    let app = spore_genetics_fixture(false).await;
    let (status, json) = get_json(
        &app,
        &format!(
            "/spore/clusters/0x{}/charts/capacity-history",
            hex::encode([2; 32])
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{json}");
}

#[derive(Clone, Copy)]
enum DecoderCellCase {
    LiveWithoutUsageMapping,
    LiveWithUnrelatedUsageMapping,
    Retired,
    NonElf,
    Duplicate,
    WrongIndexBlock,
    WrongDataHash,
    NoLocalReader,
    MissingRawTransaction,
}

async fn type_id_decoder_fixture(case: DecoderCellCase) -> axum::Router {
    use ckb_types::{
        bytes::Bytes,
        core::{BlockBuilder, EpochNumberWithFraction, TransactionBuilder},
        packed,
        prelude::*,
    };
    let store = test_store();
    let cluster = [0x77; 32];
    let args = [0x83; 32];
    let code = hex::decode(ckbadger_indexer::parser::script::TYPE_ID_CODE_HASH).unwrap();
    let type_hash = compute_script_hash(&code, 1, &args);
    let elf = include_bytes!("../../dob-decoder/tests/fixtures/nervape3x-98b7af5e.elf");
    let raw = if matches!(case, DecoderCellCase::NonElf) {
        b"retired executable".as_slice()
    } else {
        elf.as_slice()
    };
    let pattern: serde_json::Value = serde_json::from_str(include_str!(
        "../../dob-decoder/tests/fixtures/nervape3x-pattern.json"
    ))
    .unwrap();
    let description = serde_json::json!({"dob":{"ver":0,"decoder":{"type":"type_id","hash":hex::encode(args)},"pattern":pattern}}).to_string();
    let mut payload = cell(&args, 10_000_000_000_000, 1);
    payload.type_code_hash = Some(code.clone());
    payload.type_script_hash = Some(type_hash.clone());
    payload.data_size = raw.len() as i32;
    payload.occupied_capacity = (126 + raw.len() as i64) * 100_000000;
    payload.data_hash = Some(if matches!(case, DecoderCellCase::WrongDataHash) {
        vec![0x99; 32]
    } else {
        ckb_hash::blake2b_256(raw).to_vec()
    });
    let script = |code: &[u8], args: &[u8]| {
        packed::Script::new_builder()
            .code_hash(packed::Byte32::from_slice(code).unwrap())
            .hash_type(1u8.into())
            .args(Bytes::copy_from_slice(args).pack())
            .build()
    };
    let tx = TransactionBuilder::default()
        .output(
            packed::CellOutput::new_builder()
                .capacity((payload.capacity as u64).pack())
                .lock(script(&payload.lock_code_hash, &payload.lock_args))
                .type_(Some(script(&code, &args)).pack())
                .build(),
        )
        .output_data(Bytes::copy_from_slice(raw).pack())
        .build();
    let hash = tx.hash().as_slice().to_vec();
    let chain = seed_ckb_chain(&[BlockBuilder::default()
        .number(100u64.pack())
        .epoch(EpochNumberWithFraction::new(1, 0, 1800).pack())
        .transaction(tx)
        .build()]);
    let mut batch = StoreBatch::new(&store);
    batch.put_spore(
        &cluster,
        &ObjectEntry {
            standard: ObjectStandard::SporeCluster,
            collection_id: None,
            token_id: None,
            owner_lock_hash: None,
            name: Some("TypeID decoder".into()),
            description: Some(description),
            is_live: false,
            created_at_block: 99,
            created_at_tx: vec![0x78; 32],
            extra: ObjectExtra::SporeCluster,
        },
    );
    if !matches!(case, DecoderCellCase::Retired) {
        batch.put_cell(&hash, 0, &payload, 100);
        batch.put_cell_by_type(
            &type_hash,
            if matches!(case, DecoderCellCase::WrongIndexBlock) {
                99
            } else {
                100
            },
            &hash,
            0,
        );
        seed_tx(&mut batch, &hash, 100, 1);
    }
    if matches!(
        case,
        DecoderCellCase::LiveWithUnrelatedUsageMapping
            | DecoderCellCase::Retired
            | DecoderCellCase::NonElf
    ) {
        // A usage projection is neither the deployment directory nor this reader's authority.
        batch.put_script_reference_to_version(
            1,
            &type_hash,
            &if matches!(case, DecoderCellCase::LiveWithUnrelatedUsageMapping) {
                vec![0x99; 32]
            } else {
                ckb_hash::blake2b_256(elf).to_vec()
            },
        );
    }
    if matches!(case, DecoderCellCase::Duplicate) {
        batch.put_cell(&[0xff; 32], 0, &payload, 100);
        batch.put_cell_by_type(&type_hash, 100, &[0xff; 32], 0);
        seed_tx(&mut batch, &[0xff; 32], 100, 2);
    }
    batch.commit().unwrap();
    if matches!(
        case,
        DecoderCellCase::NoLocalReader | DecoderCellCase::MissingRawTransaction
    ) {
        let mut config = test_config(store);
        if matches!(case, DecoderCellCase::NoLocalReader) {
            config.ckb_db_path = "/nonexistent/ckbadger-type-id-decoder-test".into();
        }
        create_router(config).await
    } else {
        create_router(test_config_with_ckb_db_path(
            store.clone(),
            store,
            chain.path,
            Some(chain.cleanup),
        ))
        .await
    }
}

#[tokio::test]
async fn type_id_decoder_uses_current_cell_without_usage_mapping() {
    for case in [
        DecoderCellCase::LiveWithoutUsageMapping,
        DecoderCellCase::LiveWithUnrelatedUsageMapping,
    ] {
        let app = type_id_decoder_fixture(case).await;
        let (status, json) = get_json(
            &app,
            &format!("/spore/clusters/0x{}/insights", hex::encode([0x77; 32])),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        let step = &json["currentPattern"]["steps"][0];
        assert_eq!(
            step["decoderBinaryHash"],
            "0x98b7af5e071c0d4af030447b25bb436ee4e9dd687daac7bca4bee7a0393b78e1",
            "{json}"
        );
        assert_eq!(step["description"]["status"], "supported", "{json}");
        assert_eq!(step["description"]["genes"].as_array().unwrap().len(), 6);
        assert_eq!(
            step["decoderReference"]["hash"],
            format!("0x{}", hex::encode([0x83; 32]))
        );
    }
}

#[tokio::test]
async fn type_id_decoder_retirement_never_reuses_historical_binary() {
    for case in [DecoderCellCase::Retired, DecoderCellCase::NonElf] {
        let app = type_id_decoder_fixture(case).await;
        let (status, json) = get_json(
            &app,
            &format!("/spore/clusters/0x{}/insights", hex::encode([0x77; 32])),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        let step = &json["currentPattern"]["steps"][0];
        assert!(step["decoderBinaryHash"].is_null(), "{json}");
        assert_eq!(step["description"]["status"], "unsupported", "{json}");
        assert!(
            step["description"]["reason"]
                .as_str()
                .unwrap()
                .contains("current executable cell"),
            "{json}"
        );
    }
}

#[tokio::test]
async fn type_id_decoder_rejects_corrupt_current_proof_and_missing_raw_source() {
    for (case, reason) in [
        (DecoderCellCase::Duplicate, "multiple live"),
        (DecoderCellCase::WrongIndexBlock, "disagrees"),
        (DecoderCellCase::WrongDataHash, "data hash mismatch"),
        (DecoderCellCase::NoLocalReader, "requires local CKB"),
        (
            DecoderCellCase::MissingRawTransaction,
            "transaction missing in CKB store",
        ),
    ] {
        let app = type_id_decoder_fixture(case).await;
        let (status, json) = get_json(
            &app,
            &format!("/spore/clusters/0x{}/insights", hex::encode([0x77; 32])),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
        assert!(json["message"].as_str().unwrap().contains(reason), "{json}");
        assert!(
            json["message"]
                .as_str()
                .unwrap()
                .contains(&hex::encode([0x83; 32])),
            "{json}"
        );
    }
}

#[tokio::test]
async fn cluster_protocol_legacy_two_field_description_uses_owning_parser() {
    let app = cluster_protocol_fixture(4).await;
    let url = format!("/spore/clusters/0x{}", hex::encode([0x77; 32]));
    let (status, protocol) = get_json(&app, &format!("{url}/protocol")).await;
    assert_eq!(status, StatusCode::OK, "{protocol}");
    let (status, detail) = get_json(&app, &url).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(protocol["current"]["decoded"]["name"], "Genes");
    assert_eq!(
        protocol["current"]["decoded"]["description"], detail["description"],
        "{protocol}"
    );
    assert_eq!(protocol["current"]["fields"].as_array().unwrap().len(), 2);
}
