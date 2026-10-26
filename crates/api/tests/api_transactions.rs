mod common;
use common::*;

#[tokio::test]
async fn block_transaction_list_requires_node_proposal_evidence() {
    let store = test_store();
    let hash = [0xA7u8; 32];
    let second_hash = [0xA8u8; 32];
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        7,
        &CachedBlockHeader {
            hash: vec![0xB7; 32],
            parent_hash: vec![0; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
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
    batch.put_tx_hash_map(&hash, 7, 0);
    batch.put_tx_hash_map(&second_hash, 7, 1);
    batch.put_tx_index(
        7,
        0,
        &TxIndexEntry {
            tx_hash: hash,
            is_cellbase: true,
            timestamp: 1_700_000_000_000,
            inputs_count: 0,
            outputs_count: 1,
            fee: 0,
            tx_size: 100,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.put_tx_index(
        7,
        1,
        &TxIndexEntry {
            tx_hash: second_hash,
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 1,
            outputs_count: 1,
            fee: 100,
            tx_size: 100,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.commit().unwrap();
    let mut config = test_config(store);
    config.ckb_db_path = "/missing/redesign-node-test".into();
    let app = create_router(config).await;
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/transactions?block_number=7&limit=1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["message"].as_str().unwrap().contains("CKB RocksDB"));
}

#[tokio::test]
async fn committed_detail_uses_transaction_output_data_without_cell_data_cf() {
    use ckb_types::bytes::Bytes;
    use ckb_types::core::{BlockBuilder, EpochNumberWithFraction, TransactionBuilder};
    use ckb_types::packed;
    use ckb_types::prelude::*;

    let store = test_store();
    seed_genesis_baseline(&store);
    let lock = packed::Script::new_builder()
        .code_hash(packed::Byte32::new([0x11; 32]))
        .hash_type(ckb_types::core::ScriptHashType::Type.into())
        .args(Bytes::new().pack())
        .build();
    let output = packed::CellOutput::new_builder()
        .capacity(10_000_000_000u64.pack())
        .lock(lock)
        .build();
    let tx = TransactionBuilder::default()
        .output(output)
        .output_data(Bytes::from(vec![1u8, 2, 3]).pack())
        .build();
    let tx_hash: [u8; 32] = tx.hash().unpack();
    let block = BlockBuilder::default()
        .number(7u64.pack())
        .epoch(EpochNumberWithFraction::new(1, 0, 1800).pack())
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
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0x1e083126,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: 1_700_000_000_000,
        },
    );
    batch.put_tx_hash_map(&tx_hash, 7, 0);
    batch.put_tx_index(
        7,
        0,
        &TxIndexEntry {
            tx_hash,
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 0,
            outputs_count: 1,
            fee: 0,
            tx_size: 100,
            cycles: None,
            semantic_tags: 0,
        },
    );
    for output in 0..1 {
        batch.put_live_cell_marker_by_outpoint(&tx_hash, output, 7);
    }
    batch.commit().unwrap();
    let app = create_router(test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    ))
    .await;
    let (status, json) = get_json(
        &app,
        &format!("/transactions/0x{}/detail", hex::encode(tx_hash)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["outputs"][0]["commonKnowledgeSize"], "44", "{json}");
}

#[tokio::test]
async fn committed_detail_rejects_missing_indexed_input_cell() {
    use ckb_types::bytes::Bytes;
    use ckb_types::core::{BlockBuilder, EpochNumberWithFraction, TransactionBuilder};
    use ckb_types::packed;
    use ckb_types::prelude::*;

    let store = test_store();
    seed_genesis_baseline(&store);
    let lock = packed::Script::new_builder()
        .code_hash(packed::Byte32::new([0x11; 32]))
        .hash_type(ckb_types::core::ScriptHashType::Type.into())
        .args(Bytes::new().pack())
        .build();
    let output = packed::CellOutput::new_builder()
        .capacity(10_000_000_000u64.pack())
        .lock(lock)
        .build();
    let prev = packed::OutPoint::new_builder()
        .tx_hash(packed::Byte32::new([0x44; 32]))
        .index(0u32.pack())
        .build();
    let tx = TransactionBuilder::default()
        .input(packed::CellInput::new(prev, 0))
        .output(output)
        .output_data(Bytes::from(vec![1u8, 2, 3]).pack())
        .build();
    let tx_hash: [u8; 32] = tx.hash().unpack();
    let block = BlockBuilder::default()
        .number(7u64.pack())
        .epoch(EpochNumberWithFraction::new(1, 0, 1800).pack())
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
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0x1e083126,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: 1_700_000_000_000,
        },
    );
    batch.put_tx_hash_map(&tx_hash, 7, 0);
    batch.put_tx_index(
        7,
        0,
        &TxIndexEntry {
            tx_hash,
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 100,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.commit().unwrap();
    let app = create_router(test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    ))
    .await;
    let (status, json) = get_json(
        &app,
        &format!("/transactions/0x{}/detail", hex::encode(tx_hash)),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .contains("missing indexed input cell"),
        "{json}"
    );
}

#[tokio::test]
async fn block_transaction_list_rejects_missing_indexed_header() {
    let store = test_store();
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_tx_index(
        7,
        0,
        &TxIndexEntry {
            tx_hash: [0xA7; 32],
            is_cellbase: true,
            timestamp: 1_700_000_000_000,
            inputs_count: 0,
            outputs_count: 1,
            fee: 0,
            tx_size: 100,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/transactions?block_number=7")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn indexed_transaction_rejects_missing_header_on_detail_and_lifecycle() {
    let store = test_store();
    let hash = [0x91; 32];
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_tx_hash_map(&hash, 7, 0);
    batch.put_tx_index(
        7,
        0,
        &TxIndexEntry {
            tx_hash: hash,
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 1,
            outputs_count: 1,
            fee: 100,
            tx_size: 100,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    for suffix in ["", "/lifecycle"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/transactions/0x{}{suffix}",
                        hex::encode(hash)
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "{suffix}"
        );
    }
}

#[tokio::test]
async fn block_transaction_list_rejects_invalid_stored_timestamp() {
    let store = test_store();
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        7,
        &CachedBlockHeader {
            hash: vec![0xB7; 32],
            parent_hash: vec![0; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0x1e083126,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: 1_700_000_000_000,
        },
    );
    batch.put_tx_index(
        7,
        0,
        &TxIndexEntry {
            tx_hash: [0xA7; 32],
            is_cellbase: true,
            timestamp: i64::MAX,
            inputs_count: 0,
            outputs_count: 1,
            fee: 0,
            tx_size: 100,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/transactions?block_number=7")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_transaction_detail_returns_pending_mempool_transaction() {
    let store = test_store();
    // The tx-output builder reads `baseline.virtual_occupied` once per request
    // (fail-fast if absent), so a synced-chain baseline must be present.
    seed_genesis_baseline(&store);
    let server = MockServer::start().await;
    let hash = pending_tx_hash_hex();
    mount_pending_transaction_rpc(&server, &hash, "pending").await;
    // The store is empty: the input can only come from the node, as output 0
    // of the transaction that created it.
    mount_transaction_rpc(
        &server,
        funding_transaction_rpc_response("0x174876e974", &format!("0x{}", "33".repeat(20))),
    )
    .await;

    let mut config = test_config(store);
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!("/api/v1/transactions/{hash}/detail"))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["hash"], hash);
    assert_eq!(json["status"], "pending");
    assert_eq!(json["poolStatus"], "pending");
    assert_eq!(json["interpretation"]["status"], "complete");
    assert!(json["pendingSince"].as_str().is_some());
    assert_eq!(json["blockNumber"], serde_json::Value::Null);
    assert_eq!(json["blockHash"], serde_json::Value::Null);
    assert_eq!(json["index"], serde_json::Value::Null);
    assert_eq!(json["confirmations"], serde_json::Value::Null);
    assert_eq!(json["timestamp"], serde_json::Value::Null);
    assert_eq!(json["inputsCount"], 1);
    assert_eq!(json["outputsCount"], 1);
    assert_eq!(json["fee"], "372");
    // Resolved from the node, so the input side is now reported exactly:
    // 100_000_000_000 output + 372 fee, and 8 + 32 + 1 + 20 occupied bytes.
    assert_eq!(json["inputsCapacity"], "100000000372");
    assert_eq!(
        json["inputsCommonKnowledgeSize"],
        serde_json::Value::from("61")
    );
    assert_eq!(
        json["outputsCommonKnowledgeSize"],
        serde_json::Value::from("61")
    );
    assert_eq!(json["outputs"][0]["commonKnowledgeSize"], "61");
    assert_eq!(json["inputs"][0]["capacity"], "100000000372");
    assert_eq!(
        json["inputs"][0]["lock"]["codeHash"],
        TEST_SECP_LOCK_CODE_HASH
    );
    assert!(
        json["inputs"][0]["address"]
            .as_str()
            .unwrap()
            .starts_with("ckb1"),
        "a resolved input must carry its owner's address: {:?}",
        json["inputs"][0]["address"]
    );
    assert!(json["txSize"].as_i64().unwrap() > 0);
    assert_eq!(json["cycles"], 21000);
    assert_eq!(json["witnessesAvailable"], true);
    assert_eq!(
        json["witnesses"][0],
        "0x5500000010000000550000004100000000000000"
    );
    assert_eq!(
        json["inputs"][0]["previousOutput"]["txHash"],
        pending_previous_output_hash_hex()
    );
    assert_eq!(json["outputs"][0]["capacity"], "100000000000");
    // This output is at block 0 but its lock args are not the Satoshi dead
    // address, so it must NOT be tagged as a genesis special burn cell.
    assert_eq!(json["outputs"][0]["cellType"], "plain");
    assert_eq!(
        json["outputs"][0]["virtualCommonKnowledgeSize"],
        serde_json::Value::Null
    );
}

/// A POOL transaction paying the Satoshi dead-address pubkey hash is not the
/// genesis burn cell: `genesis_special_burn` (and its `virtualUsedCapacity`)
/// describe block 0's cells only. This test used to assert the opposite — the
/// pool branch passed block number 0 for every uncommitted transaction.
#[tokio::test]
async fn pending_tx_paying_the_satoshi_address_is_not_a_genesis_burn() {
    let store = test_store();
    // Seed the synced-chain baseline; `seed_genesis_baseline` uses mainnet's
    // 8.4B burnt * 6/10 == 504e15 shannons, the value asserted below.
    seed_genesis_baseline(&store);

    let server = MockServer::start().await;
    let hash = pending_tx_hash_hex();
    let satoshi_args = format!(
        "0x{}",
        hex::encode(ckbadger_common::dao::SATOSHI_PUBKEY_HASH)
    );
    let rpc_response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "transaction": {
                "hash": hash,
                "version": "0x0",
                "cell_deps": [],
                "header_deps": [],
                "inputs": [
                    {
                        "previous_output": {
                            "tx_hash": pending_previous_output_hash_hex(),
                            "index": "0x0"
                        },
                        "since": "0x0"
                    }
                ],
                "outputs": [
                    {
                        "capacity": "0x174876e800",
                        "lock": {
                            "code_hash": format!("0x{}", "11".repeat(32)),
                            "hash_type": "type",
                            "args": satoshi_args
                        },
                        "type": null
                    }
                ],
                "outputs_data": ["0x"],
                "witnesses": ["0x"]
            },
            "cycles": "0x5208",
            "fee": "0x174",
            "time_added_to_pool": pending_tx_pool_timestamp_hex(),
            "min_replace_fee": "0x175",
            "tx_status": {
                "status": "pending",
                "block_hash": null,
                "block_number": null,
                "reason": null
            }
        }
    });
    mount_transaction_rpc(&server, rpc_response).await;
    mount_transaction_rpc(
        &server,
        funding_transaction_rpc_response("0x174876e974", &format!("0x{}", "33".repeat(20))),
    )
    .await;

    let mut config = test_config(store);
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!("/api/v1/transactions/{hash}/detail"))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["outputs"][0]["cellType"], "plain");
    assert_eq!(
        json["outputs"][0]["virtualCommonKnowledgeSize"],
        serde_json::Value::Null
    );
}

/// The genesis block's cellbase, committed but not yet indexed (the store is
/// empty): its Satoshi output IS the genesis burn cell and carries the
/// network's `baseline.virtual_occupied`; its pseudo-input spends no cell, so
/// it is served (fee 0) rather than reported as unresolved.
#[tokio::test]
async fn committed_unindexed_genesis_cellbase_tags_the_satoshi_output() {
    let store = test_store();
    seed_genesis_baseline(&store);
    let server = MockServer::start().await;
    let hash = pending_tx_hash_hex();
    let cellbase_input = format!("0x{}", "00".repeat(32));
    let mut response = transaction_rpc_response(
        &hash,
        &[(&cellbase_input, u32::MAX)],
        vec![serde_json::json!({
            "capacity": "0x174876e800",
            "lock": {
                "code_hash": format!("0x{}", "11".repeat(32)),
                "hash_type": "type",
                "args": format!("0x{}", hex::encode(ckbadger_common::dao::SATOSHI_PUBKEY_HASH))
            },
            "type": null
        })],
        vec!["0x".to_string()],
        "committed",
        Some((0, &format!("0x{}", "92".repeat(32)))),
    );
    response["result"]["cycles"] = serde_json::json!("0x0");
    mount_transaction_rpc(&server, response).await;

    let mut config = test_config(store);
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;

    let (status, json) = get_json(&app, &format!("/transactions/{hash}/detail")).await;
    assert_eq!(status, StatusCode::OK, "{json:?}");
    assert_eq!(json["isCellbase"], true);
    assert_eq!(json["blockNumber"], 0);
    assert_eq!(json["fee"], "0");
    assert_eq!(json["interpretation"]["status"], "complete");
    assert_eq!(json["outputs"][0]["cellType"], "genesis_special_burn");
    assert_eq!(
        json["outputs"][0]["virtualCommonKnowledgeSize"],
        "504000000000000000"
    );
}

/// A pool transaction paying a `data2` lock (any VM2 script) is attributed to
/// that lock's address, and `/tx` renders the lock with the label the node
/// used. The mirror once mapped `data2` to byte 3 — CKB's wire value is 4 — so
/// it hashed the lock into a script hash no address has: the payee's page
/// showed nothing pending and `/tx` 500'd on the unknown byte.
#[tokio::test]
async fn pending_tx_with_data2_lock_is_attributed_to_its_address() {
    let store = test_store();
    seed_genesis_baseline(&store);
    let server = MockServer::start().await;
    let hash = pending_tx_hash_hex();

    let mut response_json = pending_transaction_rpc_response(&hash, "pending");
    response_json["result"]["transaction"]["outputs"][0]["lock"]["hash_type"] =
        serde_json::json!("data2");
    mount_transaction_rpc(&server, response_json).await;
    mount_transaction_rpc(
        &server,
        funding_transaction_rpc_response("0x174876e974", &format!("0x{}", "33".repeat(20))),
    )
    .await;
    mount_tx_pool_rpc(&server, &[(&hash, pending_tx_pool_timestamp_hex())]).await;

    let mut config = test_config(store);
    config.ckb_rpc_url = server.uri();
    let state = test_app_state(config);
    let outcome = refresh_pool_mirror_once(&state, &server.uri()).await;
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.entry_errors, 0, "{outcome:?}");
    let app = create_router_with_state(state).await;

    let payee = ckbadger_common::script_to_address(&[0x11; 32], 4, &[0x22; 20], "mainnet")
        .expect("data2 is a valid full-address hash_type");
    let (status, json) = get_json(&app, &format!("/addresses/{payee}/activities")).await;
    assert_eq!(status, StatusCode::OK, "{json:?}");
    let rows = json["data"].as_array().unwrap();
    assert_eq!(
        rows.len(),
        1,
        "the data2 payee must see its pending tx: {json:?}"
    );
    assert_eq!(rows[0]["txHash"], hash);
    assert_eq!(rows[0]["poolStatus"], "pending");

    let (status, json) = get_json(&app, &format!("/transactions/{hash}/detail")).await;
    assert_eq!(status, StatusCode::OK, "{json:?}");
    assert_eq!(json["outputs"][0]["lock"]["hashType"], "data2");
    assert_eq!(json["outputs"][0]["address"], payee);
}

/// A node that cannot answer is an error with context, not an input silently
/// reported as missing.
#[tokio::test]
async fn test_pending_transaction_fails_loudly_when_the_node_cannot_resolve_inputs() {
    let store = test_store();
    seed_genesis_baseline(&store);
    let server = MockServer::start().await;
    let hash = pending_tx_hash_hex();
    mount_pending_transaction_rpc(&server, &hash, "pending").await;
    Mock::given(method("POST"))
        .and(body_partial_json(serde_json::json!({
            "method": "get_transaction",
            "params": [pending_previous_output_hash_hex()]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": { "code": -32000, "message": "node is busy" }
        })))
        .mount(&server)
        .await;

    let mut config = test_config(store);
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/transactions/{hash}/detail"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        json["message"].as_str().unwrap().contains("node is busy"),
        "the node's own failure must reach the caller: {:?}",
        json["message"]
    );
}

/// The window between node commit and local index: the node has the
/// transaction in a block, this process's store does not have it yet. It is
/// served provisionally rather than 404, so the pool row that links here is not
/// a dead link for those seconds (hours during bulk sync).
///
/// Fixture = what a real node returns: a committed transaction carries no pool
/// `fee`, and its input is SPENT by it — no live-cell answer exists. The input
/// resolves from the parent transaction's body; the fee is Σinputs − Σoutputs.
#[tokio::test]
async fn test_committed_but_unindexed_transaction_is_served_provisionally() {
    let store = test_store();
    seed_genesis_baseline(&store);
    let server = MockServer::start().await;
    let hash = pending_tx_hash_hex();
    let mut response_json = pending_transaction_rpc_response(&hash, "committed");
    response_json["result"]["tx_status"]["block_number"] = serde_json::json!("0x1092");
    response_json["result"]["tx_status"]["block_hash"] =
        serde_json::json!(format!("0x{}", "33".repeat(32)));
    response_json["result"]["fee"] = serde_json::Value::Null;
    response_json["result"]["time_added_to_pool"] = serde_json::Value::Null;
    response_json["result"]["min_replace_fee"] = serde_json::Value::Null;
    mount_transaction_rpc(&server, response_json).await;
    mount_transaction_rpc(
        &server,
        funding_transaction_rpc_response("0x174876e974", &format!("0x{}", "33".repeat(20))),
    )
    .await;

    let mut config = test_config(store);
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;

    let (status, json) = get_json(&app, &format!("/transactions/{hash}/detail")).await;
    assert_eq!(status, StatusCode::OK, "{json:?}");
    assert_eq!(json["status"], "committed");
    assert_eq!(json["poolStatus"], "committed_awaiting_index");
    // The tx page links the committing block while indexing (lane F contract):
    // a committed response never has a null block number.
    assert_eq!(json["blockNumber"], 4242);
    assert_eq!(json["blockHash"], format!("0x{}", "33".repeat(32)));
    assert_eq!(json["timestamp"], serde_json::Value::Null);
    assert_eq!(json["confirmations"], serde_json::Value::Null);
    assert_eq!(json["interpretation"]["status"], "complete");

    let inputs_capacity: u128 = json["inputsCapacity"].as_str().unwrap().parse().unwrap();
    let outputs_capacity: u128 = json["outputsCapacity"].as_str().unwrap().parse().unwrap();
    assert_eq!(
        json["fee"],
        (inputs_capacity - outputs_capacity).to_string()
    );
    assert_eq!(json["fee"], "372");
    let input = &json["inputs"][0];
    assert_eq!(input["capacity"], "100000000372");
    assert_eq!(input["lock"]["codeHash"], TEST_SECP_LOCK_CODE_HASH);
    assert!(
        input["address"].as_str().unwrap().starts_with("ckb1"),
        "a spent input still carries its owner's address: {input:?}"
    );
}

/// With the mirror off there is no snapshot to hold a pool parent, yet a
/// chained pool spend still resolves: the node answers `get_transaction` for
/// pool transactions too.
#[tokio::test]
async fn pending_tx_whose_parent_is_also_pending_resolves_without_the_mirror() {
    let store = test_store();
    seed_genesis_baseline(&store);
    let server = MockServer::start().await;
    let hash = pending_tx_hash_hex();
    mount_pending_transaction_rpc(&server, &hash, "pending").await;
    let mut parent =
        funding_transaction_rpc_response("0x174876e974", &format!("0x{}", "33".repeat(20)));
    parent["result"]["tx_status"] = serde_json::json!({
        "status": "pending",
        "block_hash": null,
        "block_number": null,
        "reason": null
    });
    mount_transaction_rpc(&server, parent).await;

    let mut config = test_config(store);
    config.ckb_rpc_url = server.uri();
    config.pool_mirror_enabled = false;
    let app = create_router(config).await;

    let (status, json) = get_json(&app, &format!("/transactions/{hash}/detail")).await;
    assert_eq!(status, StatusCode::OK, "{json:?}");
    assert_eq!(json["poolStatus"], "pending");
    assert_eq!(json["interpretation"]["status"], "complete");
    assert_eq!(json["inputs"][0]["capacity"], "100000000372");
    assert_eq!(json["fee"], "372");
}

/// An input whose parent the node does not know cannot be resolved YET (the
/// parent was evicted, or has not propagated). The response is a retryable
/// 503, never a 200 carrying unresolved inputs and no fee.
#[tokio::test]
async fn pending_tx_with_unknown_parent_returns_503() {
    let store = test_store();
    seed_genesis_baseline(&store);
    let server = MockServer::start().await;
    let hash = pending_tx_hash_hex();
    mount_pending_transaction_rpc(&server, &hash, "pending").await;
    mount_transaction_unknown_to_node(&server, &pending_previous_output_hash_hex()).await;

    let mut config = test_config(store);
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;

    let (status, json) = get_json(&app, &format!("/transactions/{hash}/detail")).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{json:?}");
    assert_eq!(json["error"], "service_unavailable");
    let message = json["message"].as_str().unwrap();
    assert!(
        message.contains(&pending_previous_output_hash_hex()) && message.contains("retry"),
        "the 503 must name the unknown parent and say to retry: {message}"
    );
}

/// A pending Nervos DAO withdrawal completion is served with its EXACT fee:
/// Σinputs + compensation − Σoutputs, the compensation priced from the deposit
/// and request headers' accumulated rates and the request cell's own occupied
/// capacity (RFC-0023).
///
/// Numbers are the common DAO test vector
/// `compensation_uses_the_cells_actual_occupied_capacity`: a 300 CKB cell
/// occupying 142 CKB (lock args 60 bytes + DAO type + 8-byte data), AR 10_000 →
/// 11_000, compensation 15.8 CKB.
#[tokio::test]
async fn pending_dao_withdrawal_is_served_with_its_exact_fee() {
    let store = test_store();
    seed_genesis_baseline(&store);
    let server = MockServer::start().await;
    let hash = pending_tx_hash_hex();
    let request_tx = format!("0x{}", "d1".repeat(32));
    let request_block = format!("0x{}", "b1".repeat(32));
    let deposit_block_number: u64 = 1_000;
    let fee: u64 = 1_000;
    let request_capacity: u64 = 300_00000000;
    let compensation: u64 = 15_80000000;

    mount_transaction_rpc(
        &server,
        transaction_rpc_response(
            &request_tx,
            &[(&format!("0x{}", "d0".repeat(32)), 0)],
            vec![serde_json::json!({
                "capacity": format!("0x{request_capacity:x}"),
                "lock": {
                    "code_hash": TEST_SECP_LOCK_CODE_HASH,
                    "hash_type": "type",
                    "args": format!("0x{}", "44".repeat(60))
                },
                "type": {
                    "code_hash": ckbadger_indexer::parser::dao::DAO_CODE_HASH,
                    "hash_type": "type",
                    "args": "0x"
                }
            })],
            vec![format!(
                "0x{}",
                hex::encode(deposit_block_number.to_le_bytes())
            )],
            "committed",
            Some((2_000, &request_block)),
        ),
    )
    .await;
    mount_transaction_rpc(
        &server,
        transaction_rpc_response(
            &hash,
            &[(&request_tx, 0)],
            vec![serde_json::json!({
                "capacity": format!("0x{:x}", request_capacity + compensation - fee),
                "lock": {
                    "code_hash": TEST_SECP_LOCK_CODE_HASH,
                    "hash_type": "type",
                    "args": format!("0x{}", "44".repeat(20))
                },
                "type": null
            })],
            vec!["0x".to_string()],
            "pending",
            None,
        ),
    )
    .await;
    mount_header_rpc(
        &server,
        deposit_block_number,
        &format!("0x{}", "b0".repeat(32)),
        10_000,
    )
    .await;
    mount_header_rpc(&server, 2_000, &request_block, 11_000).await;

    let mut config = test_config(store);
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;

    let (status, json) = get_json(&app, &format!("/transactions/{hash}/detail")).await;
    assert_eq!(status, StatusCode::OK, "{json:?}");
    assert_eq!(json["interpretation"]["status"], "complete", "{json:?}");
    assert_eq!(json["inputs"][0]["capacity"], request_capacity.to_string());
    assert_eq!(json["fee"], fee.to_string());
}

#[tokio::test]
async fn test_transaction_detail_prefers_committed_store_over_mempool() {
    let store = test_store();
    seed_genesis_baseline(&store);
    let server = MockServer::start().await;
    let hash = pending_tx_hash_hex();
    let hash_bytes = hex::decode(hash.strip_prefix("0x").unwrap()).unwrap();
    insert_committed_transaction(&store, &hash_bytes);
    mount_pending_transaction_rpc(&server, &hash, "pending").await;

    use ckb_types::{
        core::{BlockBuilder, EpochNumberWithFraction, TransactionBuilder},
        packed,
        prelude::*,
    };
    let lock = packed::Script::new_builder()
        .code_hash(packed::Byte32::new([0x11; 32]))
        .hash_type(1u8.into())
        .args(ckb_types::bytes::Bytes::from(vec![0; 20]).pack())
        .build();
    let node_tx = TransactionBuilder::default()
        .input(
            packed::CellInput::new_builder()
                .previous_output(
                    packed::OutPoint::new_builder()
                        .tx_hash(packed::Byte32::new([0xab; 32]))
                        .index(0u32.pack())
                        .build(),
                )
                .build(),
        )
        .output(
            packed::CellOutput::new_builder()
                .capacity(10_000_000_000u64.pack())
                .lock(lock.clone())
                .build(),
        )
        .output_data(ckb_types::bytes::Bytes::new().pack())
        .build()
        .fake_hash(packed::Byte32::from_slice(&hash_bytes).unwrap());
    let mut batch = StoreBatch::new(store.as_ref());
    let cell = LiveCellInfo {
        capacity: 10_000_001_234,
        lock_script_hash: lock.calc_script_hash().as_slice().to_vec(),
        lock_code_hash: vec![0x11; 32],
        lock_hash_type: 1,
        lock_args: vec![0; 20],
        type_script_hash: None,
        type_code_hash: None,
        type_hash_type: None,
        type_args: None,
        data_size: 0,
        occupied_capacity: 6_100_000_000,
        udt_amount: None,
        data_hash: None,
    };
    batch.put_cell(&[0xab; 32], 0, &cell, 320);
    batch.put_live_cell_marker_by_outpoint(&hash_bytes, 0, 321);
    batch.commit().unwrap();
    store
        .update_tx_cycles_with_groups(
            321,
            0,
            &hash_bytes.clone().try_into().unwrap(),
            &ckbadger_common::cycles_task::TransactionCycles {
                block_hash: [0x44; 32],
                cycles: 333,
                groups: vec![ckbadger_common::cycles_task::ScriptGroupCycles {
                    script_hash: lock.calc_script_hash().as_slice().try_into().unwrap(),
                    group_type: ckbadger_common::cycles_task::ScriptGroupKind::Lock,
                    cycles: 333,
                }],
            },
        )
        .unwrap();
    let block = BlockBuilder::default()
        .number(321u64.pack())
        .epoch(EpochNumberWithFraction::new(1, 0, 1000).pack())
        .transaction(node_tx)
        .build();
    let chain = seed_ckb_chain(&[block]);
    let mut config = test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    );
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!("/api/v1/transactions/{hash}/detail"))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["hash"], hash);
    assert_eq!(json["status"], "committed");
    assert_eq!(json["blockNumber"], 321);
    assert_eq!(json["fee"], "1234");
    // `txSize` is the serialized size in block (molecule 222 + the 4-byte offset
    // slot), the size the node, explorer and wallets report — and the same size
    // `feeRate` divides by, so the two fields reproduce each other:
    // 1234 * 1000 / 226 = 5460.
    assert_eq!(json["txSize"], 226);
    assert_eq!(json["feeRate"], "5460");
    assert_eq!(
        json["feeRate"].as_str().unwrap(),
        (1234u128 * 1000 / json["txSize"].as_u64().unwrap() as u128).to_string(),
        "feeRate must be reproducible from the served txSize"
    );
    assert_eq!(json["cycles"], 333);
    assert_eq!(json["cyclesStatus"], "complete");
    assert_eq!(json["inputs"].as_array().unwrap().len(), 1);
    assert_eq!(json["outputs"][0]["status"], "live");
    let groups = json["scriptGroups"].as_array().unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0]["cycles"], "333");
    assert_eq!(groups[0]["ran"], true);
    assert!(groups[1]["cycles"].is_null());
    assert_eq!(groups[1]["ran"], false);
    assert_eq!(groups[0]["scriptHash"], groups[1]["scriptHash"]);
}

#[tokio::test]
async fn test_pending_transaction_committed_only_routes_return_explicit_error() {
    let store = test_store();
    let server = MockServer::start().await;
    let hash = pending_tx_hash_hex();
    mount_pending_transaction_rpc(&server, &hash, "pending").await;

    let mut config = test_config(store);
    config.ckb_rpc_url = server.uri();
    let state = test_app_state(config);
    state
        .pool_mirror
        .publish(healthy_pool_snapshot(vec![make_test_pool_record(
            &hex::decode(hash.trim_start_matches("0x")).unwrap(),
            &[0x11; 32],
            0,
            1,
            ckbadger_api::pool::PoolStatus::Pending,
        )]));
    let app = router_from_state(state);

    let cell_deps_request = Request::builder()
        .uri(format!("/api/v1/transactions/{hash}/cell-deps"))
        .body(Body::empty())
        .unwrap();
    let cell_deps_response = app.clone().oneshot(cell_deps_request).await.unwrap();
    assert_eq!(cell_deps_response.status(), StatusCode::BAD_REQUEST);
    let cell_deps_body = cell_deps_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let cell_deps_json: serde_json::Value = serde_json::from_slice(&cell_deps_body).unwrap();
    assert!(cell_deps_json["message"]
        .as_str()
        .unwrap()
        .contains("pending"));

    let lifecycle_request = Request::builder()
        .uri(format!("/api/v1/transactions/{hash}/lifecycle"))
        .body(Body::empty())
        .unwrap();
    let lifecycle_response = app.clone().oneshot(lifecycle_request).await.unwrap();
    assert_eq!(lifecycle_response.status(), StatusCode::BAD_REQUEST);
    let lifecycle_body = lifecycle_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let lifecycle_json: serde_json::Value = serde_json::from_slice(&lifecycle_body).unwrap();
    assert!(lifecycle_json["message"]
        .as_str()
        .unwrap()
        .contains("pending"));

    let graph_request = Request::builder()
        .uri(format!("/api/v1/graph/transaction/{hash}"))
        .body(Body::empty())
        .unwrap();
    let graph_response = app.oneshot(graph_request).await.unwrap();
    assert_eq!(graph_response.status(), StatusCode::BAD_REQUEST);
    let graph_body = graph_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let graph_json: serde_json::Value = serde_json::from_slice(&graph_body).unwrap();
    assert!(graph_json["message"].as_str().unwrap().contains("pending"));
}

#[tokio::test]
async fn test_transaction_not_found() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let hash = "0x".to_string() + &"ab".repeat(32);
    let request = Request::builder()
        .uri(format!("/api/v1/transactions/{}", hash))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// R4-E bug 3: /transactions/{hash}/lifecycle must honour proposal zones carried
// by *uncles* embedded in a window block, not just the main chain block's own
// `proposals()`. CKB consensus counts uncle proposal zones, so a tx proposed
// only inside an uncle used to report `proposedIn: null`.
// ---------------------------------------------------------------------------

/// One block of the fixture chain: its own proposal zone plus the proposal zones
/// of the uncles it embeds.
struct ProposalZone {
    number: u64,
    proposals: Vec<Vec<u8>>,
    uncles: Vec<(u64, Vec<Vec<u8>>)>,
}

fn build_fixture_block(zone: &ProposalZone) -> ckb_types::core::BlockView {
    use ckb_types::core::{BlockBuilder, EpochNumberWithFraction};
    use ckb_types::packed::ProposalShortId;
    use ckb_types::prelude::*;

    let proposal_id = |raw: &Vec<u8>| ProposalShortId::from_slice(raw).expect("10-byte short id");
    // Non-genesis headers must carry a well-formed epoch (length > index > 0-length).
    let epoch = EpochNumberWithFraction::new(1, 0, 1800);

    let mut uncle_views = Vec::new();
    for (uncle_number, uncle_proposals) in &zone.uncles {
        let mut uncle = BlockBuilder::default()
            .number(uncle_number.pack())
            .epoch(epoch.pack());
        for raw in uncle_proposals {
            uncle = uncle.proposal(proposal_id(raw));
        }
        uncle_views.push(uncle.build().as_uncle());
    }

    let mut builder = BlockBuilder::default()
        .number(zone.number.pack())
        .epoch(epoch.pack());
    for raw in &zone.proposals {
        builder = builder.proposal(proposal_id(raw));
    }
    for uncle in uncle_views {
        builder = builder.uncle(uncle);
    }
    builder.build()
}

/// Seed a committed transaction plus the [commit-10, commit-2] proposal window in
/// both stores: ckbadger block headers (hash + timestamp) and a CKB-node-format
/// RocksDB holding the real blocks.
fn seed_lifecycle_fixture(
    tx_hash: &[u8],
    commit_block: i64,
    zones: &[ProposalZone],
) -> (Arc<CkbadgerStore>, TestCkbChain) {
    use ckb_types::prelude::*;

    let store = test_store();
    let blocks: Vec<ckb_types::core::BlockView> = (commit_block - 10..=commit_block - 2)
        .filter(|number| *number >= 0)
        .map(|number| {
            zones
                .iter()
                .find(|zone| zone.number == number as u64)
                .map(build_fixture_block)
                .unwrap_or_else(|| {
                    build_fixture_block(&ProposalZone {
                        number: number as u64,
                        proposals: vec![],
                        uncles: vec![],
                    })
                })
        })
        .collect();

    let mut batch = StoreBatch::new(store.as_ref());
    let header = |hash: Vec<u8>, number: i64| CachedBlockHeader {
        hash,
        parent_hash: vec![0u8; 32],
        timestamp: 1_700_000_000_000 + number,
        epoch_number: 1,
        epoch_index: 0,
        epoch_length: 1800,
        dao: vec![0; 32],
        transactions_count: 1,
        uncles_count: 0,
        proposals_count: 0,
        compact_target: 0,
        miner_lock_hash: None,
        cycles: None,
        chain_time_ms: 1_700_000_000_000 + number,
    };
    for block in &blocks {
        let hash: [u8; 32] = block.hash().unpack();
        batch.put_block_header(
            block.number() as i64,
            &header(hash.to_vec(), block.number() as i64),
        );
    }
    batch.put_block_header(commit_block, &header(vec![0xC0; 32], commit_block));
    batch.put_tx_hash_map(tx_hash, commit_block, 0);
    batch.put_tx_index(
        commit_block,
        0,
        &TxIndexEntry {
            tx_hash: tx_hash
                .try_into()
                .expect("fixture transaction hash is 32 bytes"),
            is_cellbase: false,
            timestamp: 1_700_000_000_000 + commit_block,
            inputs_count: 1,
            outputs_count: 1,
            fee: 1000,
            tx_size: 500,
            cycles: Some(1000),
            semantic_tags: 0,
        },
    );
    batch.put_tx_actions(&make_test_tx_actions(
        &[0x21; 32],
        tx_hash,
        &[0xC0; 32],
        commit_block,
        0,
        1000,
        0,
    ));
    batch.put_cell(
        tx_hash,
        0,
        &LiveCellInfo {
            capacity: 10_000_000_000,
            lock_script_hash: vec![0x21; 32],
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![0; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 6_100_000_000,
            udt_amount: None,
            data_hash: None,
        },
        commit_block,
    );
    batch.commit().unwrap();
    store
        .update_sync_status(|s| {
            s.tip_block_number = commit_block + 100;
        })
        .unwrap();

    let chain = seed_ckb_chain(&blocks);
    (store, chain)
}

#[tokio::test]
async fn lifecycle_rejects_an_unavailable_node_reader() {
    let tx_hash = vec![0x7b; 32];
    let (store, _chain) = seed_lifecycle_fixture(&tx_hash, 100, &[]);
    let mut config = test_config(store);
    config.ckb_db_path = "/nonexistent/ckbadger-review-node-db".into();
    let app = create_router(config).await;
    let (status, json) = get_json(
        &app,
        &format!("/transactions/0x{}/lifecycle", hex::encode(&tx_hash)),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .contains("requires the CKB node DB reader"),
        "{json}"
    );
}

#[tokio::test]
async fn lifecycle_rejects_missing_or_malformed_proposal_headers() {
    let tx_hash = vec![0x7b; 32];
    for malformed in [false, true] {
        let (store, chain) = seed_lifecycle_fixture(&tx_hash, 100, &[]);
        store
            .delete_cf(
                store.cf_block_headers(),
                &ckbadger_store::keys::encode_block_num(90),
            )
            .unwrap();
        if malformed {
            let mut header = store.get_block_header(100).unwrap().unwrap();
            header.hash = vec![1, 2];
            let mut batch = StoreBatch::new(&store);
            batch.put_block_header(90, &header);
            batch.commit().unwrap();
        }
        let config = test_config_with_ckb_db_path(
            store.clone(),
            store,
            chain.path.clone(),
            Some(chain.cleanup.clone()),
        );
        let app = create_router(config).await;
        let (status, json) = get_json(
            &app,
            &format!("/transactions/0x{}/lifecycle", hex::encode(&tx_hash)),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
        assert!(json["message"].as_str().unwrap().contains("90"), "{json}");
    }
}

#[tokio::test]
async fn test_transaction_lifecycle_honours_uncle_proposal_zone() {
    let tx_hash = vec![0x7b; 32];
    let short_id = tx_hash[..10].to_vec();
    let other_id = vec![0x9f; 10];
    let commit_block = 442i64;

    // The tx's short id appears ONLY in the proposals of an uncle (#438) embedded
    // in main block 440; block 440's own proposal zone holds a different id.
    let zones = vec![
        ProposalZone {
            number: 434,
            proposals: vec![],
            uncles: vec![],
        },
        ProposalZone {
            number: 437,
            proposals: vec![other_id.clone()],
            uncles: vec![],
        },
        ProposalZone {
            number: 440,
            proposals: vec![other_id.clone()],
            uncles: vec![(438, vec![short_id.clone()])],
        },
    ];
    let (store, chain) = seed_lifecycle_fixture(&tx_hash, commit_block, &zones);

    let config = test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    );
    let app = create_router(config).await;

    let (status, json) = get_json(
        &app,
        &format!("/transactions/0x{}/lifecycle", hex::encode(&tx_hash)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["phase"], "committed");
    assert_eq!(
        json["proposedIn"]["blockNumber"], 440,
        "uncle-borne proposals belong to the main-chain block that embeds the uncle, got {}",
        json["proposedIn"]
    );
    assert_eq!(json["commitmentDistance"], commit_block - 440);
    assert_eq!(json["proposedInUncle"]["blockNumber"], 438);
    assert_eq!(json["committedIn"]["blockNumber"], commit_block);
    let (status, rows) =
        get_json(&app, &format!("/transactions?block_number={commit_block}")).await;
    assert_eq!(status, StatusCode::OK, "{rows}");
    assert_eq!(rows["data"][0]["proposalOrigin"]["blockNumber"], 440);
    assert_eq!(rows["data"][0]["proposalOrigin"]["distance"], 2);
    assert_eq!(rows["data"][0]["proposalOrigin"]["uncle"]["number"], 438);
    assert_eq!(rows["data"][0]["outputCapacity"], "10000000000");
    assert_eq!(rows["data"][0]["cycles"], 1000);
    assert_eq!(rows["data"][0]["cyclesStatus"], "complete");
}

#[tokio::test]
async fn test_transaction_lifecycle_reports_earliest_main_proposal() {
    // Control: a tx proposed directly in the main proposal zone still reports the
    // earliest window block, and carries no uncle attribution.
    let tx_hash = vec![0x3c; 32];
    let short_id = tx_hash[..10].to_vec();
    let commit_block = 442i64;

    let zones = vec![
        ProposalZone {
            number: 434,
            proposals: vec![],
            uncles: vec![],
        },
        ProposalZone {
            number: 435,
            proposals: vec![short_id.clone()],
            uncles: vec![],
        },
        ProposalZone {
            number: 437,
            proposals: vec![short_id.clone()],
            uncles: vec![],
        },
    ];
    let (store, chain) = seed_lifecycle_fixture(&tx_hash, commit_block, &zones);

    let config = test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    );
    let app = create_router(config).await;

    let (status, json) = get_json(
        &app,
        &format!("/transactions/0x{}/lifecycle", hex::encode(&tx_hash)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["proposedIn"]["blockNumber"], 435);
    assert_eq!(json["commitmentDistance"], commit_block - 435);
    assert_eq!(json["proposedInUncle"], serde_json::Value::Null);
}

#[tokio::test]
async fn test_transaction_lifecycle_prefers_main_zone_over_uncle_in_same_block() {
    // When one block proposes the tx both directly and through an uncle, the
    // direct main-chain proposal owns the attribution.
    let tx_hash = vec![0x5e; 32];
    let short_id = tx_hash[..10].to_vec();
    let commit_block = 442i64;

    let zones = vec![ProposalZone {
        number: 436,
        proposals: vec![short_id.clone()],
        uncles: vec![(433, vec![short_id.clone()])],
    }];
    let (store, chain) = seed_lifecycle_fixture(&tx_hash, commit_block, &zones);

    let config = test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    );
    let app = create_router(config).await;

    let (status, json) = get_json(
        &app,
        &format!("/transactions/0x{}/lifecycle", hex::encode(&tx_hash)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["proposedIn"]["blockNumber"], 436);
    assert_eq!(json["proposedInUncle"], serde_json::Value::Null);
}

// ---------------------------------------------------------------------------
// Audited bug (2026-08-01 night, agent E): /transactions/{hash}/cell-deps
// answered `200 []` both when the CKB RocksDB reader was unavailable and when
// the transaction did not exist — a silent-empty shape that made "no deps",
// "no reader", and "no such tx" indistinguishable. Fail-fast contract: reader
// unavailable -> 5xx with context; tx nonexistent -> 404.
// ---------------------------------------------------------------------------

fn unknown_transaction_rpc_response() -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "transaction": null,
            "cycles": null,
            "fee": null,
            "min_replace_fee": null,
            "time_added_to_pool": null,
            "tx_status": {
                "status": "unknown",
                "block_hash": null,
                "block_number": null,
                "reason": null
            }
        }
    })
}

async fn mount_unknown_transaction_rpc(server: &MockServer) {
    Mock::given(method("POST"))
        .and(body_partial_json(
            serde_json::json!({ "method": "get_transaction" }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(unknown_transaction_rpc_response()))
        .mount(server)
        .await;
}

#[tokio::test]
async fn test_cell_deps_nonexistent_tx_returns_404() {
    let store = test_store();
    let server = MockServer::start().await;
    mount_unknown_transaction_rpc(&server).await;
    let hash = format!("0x{}", "ee".repeat(32));

    let mut config = test_config(store);
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;

    let (status, json) = get_json(&app, &format!("/transactions/{hash}/cell-deps")).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a transaction unknown to both the CKB store and the node must 404, not answer a silent `200 []`, got {json}"
    );
    assert!(
        json["message"]
            .as_str()
            .unwrap_or_default()
            .contains("Transaction not found"),
        "got {json}"
    );
}

#[tokio::test]
async fn test_cell_deps_without_ckb_store_is_5xx_not_silent_empty() {
    let store = test_store();
    // The RPC mock reports the tx as unknown so the OLD code path (RPC lookup
    // then `ok(vec![])`) observably produced `200 []` here rather than failing
    // on an unreachable RPC endpoint.
    let server = MockServer::start().await;
    mount_unknown_transaction_rpc(&server).await;

    // A CKB DB path that does not exist: the reader cannot open, so the
    // endpoint has no data source at all.
    let missing_path =
        std::env::temp_dir().join(format!("ckbadger-missing-ckb-db-{}", Uuid::new_v4()));
    let mut config = test_config_with_ckb_db_path(
        store.clone(),
        store,
        missing_path.to_string_lossy().to_string(),
        None,
    );
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;

    let (status, json) = get_json(
        &app,
        &format!("/transactions/0x{}/cell-deps", "ee".repeat(32)),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "no CKB RocksDB reader means the endpoint has no data source: loud 5xx, never a silent `200 []`, got {json}"
    );
    assert!(
        json["message"]
            .as_str()
            .unwrap_or_default()
            .contains("CKB RocksDB reader"),
        "error must name the unavailable reader, got {json}"
    );
}

#[tokio::test]
async fn test_cell_deps_committed_tx_returns_deps_from_ckb_store() {
    use ckb_types::prelude::*;

    let store = test_store();

    let code = ckb_types::core::TransactionBuilder::default()
        .output(ckb_types::packed::CellOutput::default())
        .output_data([1u8, 2, 3].pack())
        .build();
    let code_hash = code.hash();
    let group_data = ckb_types::packed::OutPointVec::new_builder()
        .push(
            ckb_types::packed::OutPoint::new_builder()
                .tx_hash(code_hash.clone())
                .index(0u32.pack())
                .build(),
        )
        .build();
    let group = ckb_types::core::TransactionBuilder::default()
        .output(ckb_types::packed::CellOutput::default())
        .output_data(Vec::<u8>::new().pack())
        .output(ckb_types::packed::CellOutput::default())
        .output_data(group_data.as_bytes().pack())
        .build();
    let dep_tx_hash: [u8; 32] = group.hash().unpack();
    let tx = ckb_types::core::TransactionBuilder::default()
        .cell_dep(
            ckb_types::packed::CellDep::new_builder()
                .out_point(
                    ckb_types::packed::OutPoint::new_builder()
                        .tx_hash(ckb_types::packed::Byte32::new(dep_tx_hash))
                        .index(1u32.pack())
                        .build(),
                )
                .dep_type(ckb_types::core::DepType::DepGroup.into())
                .build(),
        )
        .build();
    let tx_hash: [u8; 32] = tx.hash().unpack();

    let epoch = ckb_types::core::EpochNumberWithFraction::new(1, 0, 1800);
    let block = ckb_types::core::BlockBuilder::default()
        .number(77u64.pack())
        .epoch(epoch.pack())
        .transaction(code)
        .transaction(group)
        .transaction(tx)
        .build();
    let chain = seed_ckb_chain(&[block]);

    let config = test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    );
    let app = create_router(config).await;

    let (status, json) = get_json(
        &app,
        &format!("/transactions/0x{}/cell-deps", hex::encode(tx_hash)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "got {json}");
    let deps = json.as_array().expect("cell deps array");
    assert_eq!(deps.len(), 1);
    assert_eq!(
        deps[0]["outPointTxHash"],
        format!("0x{}", hex::encode(dep_tx_hash))
    );
    assert_eq!(deps[0]["outPointIndex"], 1);
    assert_eq!(deps[0]["depType"], "dep_group");
    assert_eq!(
        deps[0]["resolvedCells"][0]["txHash"],
        format!("0x{}", hex::encode(code_hash.as_slice()))
    );
    assert_eq!(deps[0]["resolvedCells"][0]["index"], 0);
    assert_eq!(
        deps[0]["resolvedCells"][0]["dataHash"],
        format!(
            "0x{}",
            hex::encode(ckb_types::packed::CellOutput::calc_data_hash(&[1u8, 2, 3]).as_slice())
        )
    );
    assert_eq!(deps[0]["scriptHashes"], serde_json::json!([]));
}

// ── `.cell` records decoded from the creating transaction's witness ──────
// Chain data copied from `crates/indexer/src/parser/dotcell_fixtures.rs`
// `T2_REGISTER_JOAOM`: testnet tx 0x89191ea4…386b at block 22471181, which
// registers `joaom.cell` (output 1, no records) and re-creates its ring
// predecessor `maria.cell` (output 0, six records in witness 0). Node-verified
// 2026-09-24. The fixture module is `#[cfg(test)]` in the indexer crate.

const T2_ACCOUNT_LOCK: &str = "ede6a3d80717c3d7927eea678d095abbe68dbb08ca6fdbbbdd9de906455a4afd";
const T2_ACCOUNT_TYPE: &str = "e0706b176678181d982290d93dfcd82098e60cceaa4a87f10f32dcbcc91df1d9";
const T2_NAMESPACE_ARGS: &str = "2510c78057479c9b023fe6e98ce43979e92a1353";
/// `T2_OUT0_DATA` (`maria.cell`).
const T2_OUT0_DATA: &str = "033b339494bd0e29b77c0dca959bc4d6d87e4ac232bd7df9c1335163fe85f5eb18241e3586a41eb75dd6d68bf555acea74d6649ed529da856c0058e6c6f873af57732daae458be3c56c2c847b14158e6c6f873af57732daae458be3c56c2c847b1416d61726961";
/// `T2_OUT1_DATA` (`joaom.cell`).
const T2_OUT1_DATA: &str = "0372ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e22cf2cdac7ab0e7b97f4b50475fb5ce1b32dfe711fbe68f6c0069e8165efb4cb3b2cd62300e7d16f41a1c65ceab69e8165efb4cb3b2cd62300e7d16f41a1c65ceab6a6f616f6d";
/// `T2_WITNESS_0`: maria's six records in `output_type`.
const T2_WITNESS_0: &str = "c901000010000000100000001c000000080000007265676973746572a901000006000b616464726573732e333039006400636b7431717266727763646e76737373776477706e337339763866703837656d617433303663746a77736d336e6d6c6b6a673871797a61326371677171397837357a75346c37676c64363036723665796430306d346c7a79337a6b786b71346e79777a752c01000009616464726573732e30003e00746231703237767464306a776d3235746478336d36746763757168346c6578756c3076736c7572683079637a633766393272667030736b737264737936742c0100000a616464726573732e3630002a003078656646324634436132444536656444363364416665343833383536463134453639346437333144442c0100000d70726f66696c652e656d61696c0011006d61726961406578616d706c652e636f6d2c0100000d70726f66696c652e70686f6e650010002b3335312039313220333435203637382c0100000a647765622e636b626673004800636b6266733a2f2f346266306362646261633066386538656231616664646534333963336263306234333731623366336165636538633730343866656630366566366563376231302c010000";
/// `T2_WITNESS_1`: a secp witness whose `output_type` is joaom's empty records payload.
const T2_WITNESS_1: &str = "7f00000010000000550000007900000041000000e329de7a51feb20409c370832f99a08b76236b2c6cdbb7cc5bbc2858d1bc69b753a8b932eaa752fa46c794eaa36adee5cd063a25760cbd0fa151034f6b0e66010020000000b749b13ab9026ab71f3cb0cc971d0dbf1030bae1f8b3a006a45055d5eec73f0a020000000000";

/// The T2 transaction rebuilt from its chain bytes. `cell_deps` are not part of
/// the fixture, so the hash differs from the chain's; nothing here reads it
/// beyond looking the transaction up by it.
fn t2_register_transaction() -> ckb_types::core::TransactionView {
    use ckb_types::bytes::Bytes;
    use ckb_types::core::{Capacity, ScriptHashType, TransactionBuilder};
    use ckb_types::packed;
    use ckb_types::prelude::*;

    let script = |code_hash: &str, args: &str| {
        let code_hash: [u8; 32] = hex::decode(code_hash).unwrap().try_into().unwrap();
        packed::Script::new_builder()
            .code_hash(packed::Byte32::new(code_hash))
            .hash_type(ScriptHashType::Type.into())
            .args(Bytes::from(hex::decode(args).unwrap()).pack())
            .build()
    };
    let output = |capacity: u64, lock: packed::Script, type_: Option<packed::Script>| {
        packed::CellOutput::new_builder()
            .capacity(Capacity::shannons(capacity).pack())
            .lock(lock)
            .type_(type_.pack())
            .build()
    };
    let input = |tx_hash: &str, index: u32| {
        let tx_hash: [u8; 32] = hex::decode(tx_hash).unwrap().try_into().unwrap();
        packed::CellInput::new(
            packed::OutPoint::new_builder()
                .tx_hash(packed::Byte32::new(tx_hash))
                .index(index.pack())
                .build(),
            0,
        )
    };
    let bytes = |hex_str: &str| Bytes::from(hex::decode(hex_str).unwrap()).pack();
    let name_lock = || script(T2_ACCOUNT_LOCK, "");
    let name_type = || Some(script(T2_ACCOUNT_TYPE, T2_NAMESPACE_ARGS));

    TransactionBuilder::default()
        .input(input(
            "974fc983a62a6f7b977c6e2170695cc4aafc5a7544c1aa2513574f1563fa0fad",
            0,
        ))
        .input(input(
            "b1745c34666bed64ec8654b16ca3d34af815576ad8cad7330957b3a54c9b34ae",
            0,
        ))
        .input(input(
            "b1745c34666bed64ec8654b16ca3d34af815576ad8cad7330957b3a54c9b34ae",
            1,
        ))
        .output(output(0x59682f000, name_lock(), name_type()))
        .output_data(bytes(T2_OUT0_DATA))
        .output(output(0x59682f000, name_lock(), name_type()))
        .output_data(bytes(T2_OUT1_DATA))
        .output(output(
            0x5c89ddb680,
            script(
                "d23761b364210735c19c60561d213fb3beae2fd6172743719eff6920e020baac",
                "000140911fa94eaef8c1d0eca81b23e1972ecb0548dc",
            ),
            None,
        ))
        .output_data(bytes(""))
        .output(output(
            0x19f6a3c11688,
            script(
                "9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
                "23870b08ec5f6260c50a63646170d61e26d155c7",
            ),
            None,
        ))
        .output_data(bytes(""))
        .witness(bytes(T2_WITNESS_0))
        .witness(bytes(T2_WITNESS_1))
        .build()
}

async fn dotcell_transaction_app(index_identities: bool) -> (axum::Router, String) {
    use ckb_types::prelude::*;
    use ckbadger_indexer::parser::dotcell::DotCellParser;

    let block_number: i64 = 22_471_181;
    let tx = t2_register_transaction();
    let tx_hash: [u8; 32] = tx.hash().unpack();
    let block = ckb_types::core::BlockBuilder::default()
        .number((block_number as u64).pack())
        .epoch(ckb_types::core::EpochNumberWithFraction::new(1, 0, 1800).pack())
        .transaction(tx)
        .build();
    let block_hash: [u8; 32] = block.hash().unpack();
    let chain = seed_ckb_chain(&[block]);

    let store = test_store();
    seed_genesis_baseline(&store);
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        block_number,
        &CachedBlockHeader {
            hash: block_hash.to_vec(),
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 1,
            epoch_index: 0,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: 1_700_000_000_000,
        },
    );
    batch.put_tx_hash_map(&tx_hash, block_number, 0);
    batch.put_tx_index(
        block_number,
        0,
        &TxIndexEntry {
            tx_hash,
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 3,
            outputs_count: 4,
            fee: 1000,
            tx_size: 1500,
            cycles: Some(1000),
            semantic_tags: 0,
        },
    );
    // The committed transaction's three inputs resolve through the domain
    // consumed markers and immutable cell payloads, as they do after indexing.
    let input_cell = LiveCellInfo {
        capacity: 10_000_000_000,
        lock_script_hash: vec![0x11; 32],
        lock_code_hash: vec![0x22; 32],
        lock_hash_type: 1,
        lock_args: vec![],
        type_script_hash: None,
        type_code_hash: None,
        type_hash_type: None,
        type_args: None,
        data_size: 0,
        occupied_capacity: 41,
        udt_amount: None,
        data_hash: None,
    };
    for (hash_hex, output_index) in [
        (
            "974fc983a62a6f7b977c6e2170695cc4aafc5a7544c1aa2513574f1563fa0fad",
            0,
        ),
        (
            "b1745c34666bed64ec8654b16ca3d34af815576ad8cad7330957b3a54c9b34ae",
            0,
        ),
        (
            "b1745c34666bed64ec8654b16ca3d34af815576ad8cad7330957b3a54c9b34ae",
            1,
        ),
    ] {
        batch.put_consumed_cell(
            &hex::decode(hash_hex).unwrap(),
            output_index,
            &input_cell,
            block_number - 1,
            block_number,
        );
    }
    for output in 0..4 {
        batch.put_live_cell_marker_by_outpoint(&tx_hash, output, block_number);
    }
    if index_identities {
        // Both live insert_dotcell_name and bulk ObjectOwner::insert_dotcell
        // persist ordinary names in CF_IDENTITY_DATA. Only the empty-label
        // ring root is infrastructure without an identity. Derive these rows
        // from the same cell/witness bytes instead of inventing display labels.
        let namespace_args = hex::decode(T2_NAMESPACE_ARGS).unwrap().try_into().unwrap();
        for (data, witness) in [(T2_OUT0_DATA, T2_WITNESS_0), (T2_OUT1_DATA, T2_WITNESS_1)] {
            let name = DotCellParser::parse_name_data(&hex::decode(data).unwrap()).unwrap();
            let records =
                DotCellParser::parse_witness_records(witness, &name.records_hash).unwrap();
            assert!(!name.is_root());
            batch.put_identity(
                &name.id,
                &IdentityEntry {
                    standard: IdentityStandard::DotCell,
                    owner_lock_hash: None,
                    name: Some(format!("{}.cell", name.label)),
                    is_live: true,
                    created_at_block: block_number,
                    created_at_tx: tx_hash.to_vec(),
                    extra: IdentityExtra::DotCell {
                        label: name.label.clone(),
                        namespace_args,
                        layout_version: name.layout_version,
                        expired_at: name.expired_at,
                        owner_hash20: name.owner_hash20,
                        manager_hash20: name.manager_hash20,
                        next_id: name.next_id,
                        records_hash: name.records_hash,
                        records,
                        parent_id: name.parent_id(),
                    },
                },
            );
        }
    }
    batch.commit().unwrap();
    store
        .update_sync_status(|s| {
            s.tip_block_number = block_number + 10;
        })
        .unwrap();

    let mut config = test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    );
    config.ckb_network = "testnet".to_string();
    let app = create_router(config).await;
    (
        app,
        format!("/transactions/0x{}/detail", hex::encode(tx_hash)),
    )
}

/// The tx page decodes each `.cell` name's records where they live — the
/// witness at the name cell's own output index — through the same parser the
/// indexer runs, and resolves the `address.309` record to a testnet address.
#[tokio::test]
async fn test_transaction_detail_decodes_dotcell_records_from_witness() {
    let (app, url) = dotcell_transaction_app(true).await;
    let (status, json) = get_json(&app, &url).await;
    assert_eq!(status, StatusCode::OK, "got {json}");
    let names = json["dotcellNames"]
        .as_array()
        .unwrap_or_else(|| panic!("dotcellNames missing: {json}"));
    assert_eq!(names.len(), 2, "both name outputs: {json}");

    let maria = &names[0];
    assert_eq!(maria["outputIndex"], 0);
    assert_eq!(maria["name"], "maria.cell");
    assert_eq!(
        maria["identityId"],
        "0x2224948f63975a7a0741139cd5d2a45b9fb02c03"
    );
    let records = maria["records"].as_array().expect("records");
    assert_eq!(records.len(), 6);
    assert_eq!(records[0]["key"], "address.309");
    assert!(
        records[0]["decodedAddress"]["address"]
            .as_str()
            .is_some_and(|address| address.starts_with("ckt1")),
        "got {}",
        records[0]
    );

    let joaom = &names[1];
    assert_eq!(joaom["outputIndex"], 1);
    assert_eq!(joaom["name"], "joaom.cell");
    assert_eq!(
        joaom["identityId"],
        "0x241e3586a41eb75dd6d68bf555acea74d6649ed5"
    );
    assert_eq!(joaom["records"].as_array().map(Vec::len), Some(0));

    for (output_index, name) in [maria, joaom].into_iter().enumerate() {
        let display = &json["outputs"][output_index]["display"];
        assert_eq!(display["kind"], "identity");
        assert_eq!(display["standard"], "dotcell");
        assert_eq!(display["id"], name["identityId"]);
        assert_eq!(display["name"], name["name"]);
    }
}

#[tokio::test]
async fn committed_dotcell_detail_rejects_missing_indexed_identity_metadata() {
    let (app, url) = dotcell_transaction_app(false).await;
    let (status, json) = get_json(&app, &url).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    let message = json["message"].as_str().expect("error message");
    assert!(
        message.contains("missing indexed identity metadata")
            && message.contains("0x2224948f63975a7a0741139cd5d2a45b9fb02c03")
            && message.contains("output_index=0")
            && message.contains(
                url.trim_start_matches("/transactions/")
                    .trim_end_matches("/detail")
            ),
        "{message}"
    );
}

/// The pending (pool) builder decodes `.cell` records through the same call:
/// a name registration waiting in the pool already shows its records.
#[tokio::test]
async fn test_pending_transaction_detail_decodes_dotcell_records_from_witness() {
    let store = test_store();
    seed_genesis_baseline(&store);
    let server = MockServer::start().await;
    let hash = pending_tx_hash_hex();

    // The standard pending fixture, its one output replaced by T2's
    // `maria.cell` with the witness carrying maria's records.
    let mut response = pending_transaction_rpc_response(&hash, "pending");
    let tx = &mut response["result"]["transaction"];
    tx["outputs"] = serde_json::json!([{
        "capacity": "0x59682f000",
        "lock": {
            "code_hash": format!("0x{T2_ACCOUNT_LOCK}"),
            "hash_type": "type",
            "args": "0x"
        },
        "type": {
            "code_hash": format!("0x{T2_ACCOUNT_TYPE}"),
            "hash_type": "type",
            "args": format!("0x{T2_NAMESPACE_ARGS}")
        }
    }]);
    tx["outputs_data"] = serde_json::json!([format!("0x{T2_OUT0_DATA}")]);
    tx["witnesses"] = serde_json::json!([format!("0x{T2_WITNESS_0}")]);
    mount_transaction_rpc(&server, response).await;
    mount_transaction_rpc(
        &server,
        funding_transaction_rpc_response("0x174876e974", &format!("0x{}", "33".repeat(20))),
    )
    .await;

    let mut config = test_config(store);
    config.ckb_rpc_url = server.uri();
    config.ckb_network = "testnet".to_string();
    let app = create_router(config).await;

    let (status, json) = get_json(&app, &format!("/transactions/{hash}/detail")).await;
    assert_eq!(status, StatusCode::OK, "got {json}");
    assert_eq!(json["status"], "pending");
    let names = json["dotcellNames"]
        .as_array()
        .unwrap_or_else(|| panic!("dotcellNames missing: {json}"));
    assert_eq!(names.len(), 1);
    assert_eq!(names[0]["outputIndex"], 0);
    assert_eq!(names[0]["name"], "maria.cell");
    let records = names[0]["records"].as_array().expect("records");
    assert_eq!(records.len(), 6);
    assert!(
        records[0]["decodedAddress"]["address"]
            .as_str()
            .is_some_and(|address| address.starts_with("ckt1")),
        "got {}",
        records[0]
    );
}

/// A transaction the store has indexed whose cycles are not computed yet: the
/// row the cycles long-poll waits on.
fn seed_tx_awaiting_cycles(store: &Arc<CkbadgerStore>, tx_hash: &[u8; 32], cycles: Option<i64>) {
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        321,
        &CachedBlockHeader {
            hash: vec![0x44; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_123_456,
            epoch_number: 1,
            epoch_index: 0,
            epoch_length: 1000,
            dao: vec![0; 32],
            transactions_count: 2,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: 1_700_000_123_456,
        },
    );
    batch.put_tx_hash_map(tx_hash, 321, 1);
    batch.put_tx_index(
        321,
        1,
        &TxIndexEntry {
            tx_hash: *tx_hash,
            is_cellbase: false,
            timestamp: 1_700_000_123_456,
            inputs_count: 1,
            outputs_count: 1,
            fee: 1234,
            tx_size: 222,
            cycles,
            semantic_tags: 0,
        },
    );
    batch.commit().unwrap();
}

/// The cycles long-poll waits on purpose for the indexer's cycles worker to
/// write the result; that wait is not a slow request.
#[tokio::test]
async fn test_cycles_long_poll_wait_is_not_a_slow_request() {
    // The threshold, and how long the worker takes to answer after the handler
    // has enqueued: the request lasts at least that long in wall time. What
    // the handler does besides sleeping is the host's to decide; it only has
    // to stay under the threshold.
    const SLOW_MS: u64 = 1_500;
    let store = test_store();
    let tx_hash = [0x5A; 32];
    let hash_hex = format!("0x{}", hex::encode(tx_hash));
    seed_tx_awaiting_cycles(&store, &tx_hash, None);
    let cycles_dir = tempfile::tempdir().unwrap();
    let mut config = test_config(store.clone());
    config.cycles_request_dir = Some(cycles_dir.path().to_path_buf());
    config.slow_request_threshold_ms = SLOW_MS;
    let app = create_router(config).await;
    let logs = LogCapture::start();

    let uri = format!("/api/v1/transactions/{hash_hex}/calculate-cycles");
    let request = {
        let app = app.clone();
        let uri = uri.clone();
        tokio::spawn(async move {
            let response = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(uri)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let body = response.into_body().collect().await.unwrap().to_bytes();
            let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
            (status, json)
        })
    };

    // The handler has asked the cycles worker; the worker answers a whole
    // threshold later.
    let request_file = cycles_dir.path().join(format!("tx_{hash_hex}"));
    let started = std::time::Instant::now();
    while !request_file.exists() {
        assert!(
            // A hang guard, not a latency bound: generous for a loaded host.
            started.elapsed() < std::time::Duration::from_secs(30),
            "the handler never enqueued the cycles request"
        );
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(SLOW_MS)).await;
    store
        .update_tx_cycles_with_groups(
            321,
            1,
            &tx_hash,
            &ckbadger_common::cycles_task::TransactionCycles {
                block_hash: [0x44; 32],
                cycles: 777,
                groups: vec![ckbadger_common::cycles_task::ScriptGroupCycles {
                    script_hash: [0x21; 32],
                    group_type: ckbadger_common::cycles_task::ScriptGroupKind::Lock,
                    cycles: 777,
                }],
            },
        )
        .unwrap();

    let (status, json) = request.await.unwrap();
    assert_eq!(status, StatusCode::OK, "got {json}");
    assert_eq!(json["status"], "done", "{json}");
    assert_eq!(json["cycles"], 777);

    let path = format!("path=\"{uri}\"");
    assert!(
        logs.lines_with(&[&path, "slow request"]).is_empty(),
        "the long-poll wait was logged as a slow request: {:#?}",
        logs.lines_with(&[&path])
    );
    let completed = logs.lines_with(&[&path, "completed"]);
    assert_eq!(completed.len(), 1, "{completed:#?}");
    let latency_ms = LogCapture::field(&completed[0], "latency_ms");
    let waited_ms = LogCapture::field(&completed[0], "waited_ms");
    // In wall time the request reached the threshold (it enqueued before the
    // test's sleep and answered `done` only after the worker wrote) ...
    assert!(
        latency_ms >= SLOW_MS,
        "the request outlasted the threshold: {}",
        completed[0]
    );
    // ... and part of it was the deliberate wait.
    assert!(
        waited_ms > 0 && waited_ms <= latency_ms,
        "the log reports the wait within the latency: {}",
        completed[0]
    );
}

#[tokio::test]
async fn redesign_pool_pages_newest_first_with_exact_values_and_interpretation() {
    use ckbadger_api::pool::PoolStatus;
    let state = test_app_state(test_config(test_store()));
    state.pool_mirror.publish(healthy_pool_snapshot(vec![
        make_test_pool_record(&[0x11; 32], &[0x21; 32], 700, 1000, PoolStatus::Pending),
        make_test_pool_record(&[0x12; 32], &[0x21; 32], -800, 2000, PoolStatus::Proposed),
    ]));
    let app = router_from_state(state);
    let (status, first) = get_json(&app, "/transactions/pool?limit=1").await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["data"][0]["hash"], format!("0x{}", "12".repeat(32)));
    assert_eq!(first["data"][0]["value"], "0");
    assert_eq!(first["data"][0]["interpretation"]["status"], "complete");
    assert_eq!(first["totals"]["pending"], 1);
    assert_eq!(first["totals"]["proposed"], 1);
    assert_eq!(first["data"][0]["feeRate"], "2000");
    assert_eq!(
        first["summary"],
        serde_json::json!({"size":"1000","cycles":"400000","minFeeRate":"2000","maxFeeRate":"2000","newestSeenAt":"2000"})
    );
    let cursor = first["nextCursor"].as_str().unwrap();
    let (status, second) =
        get_json(&app, &format!("/transactions/pool?limit=1&cursor={cursor}")).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["data"][0]["value"], "700");
    assert_eq!(second["summary"], first["summary"]);
    assert!(second["nextCursor"].is_null());
    assert!(
        second["data"][0]["activity"].get("blockNumber").is_none(),
        "pool activity must not invent a committing block"
    );
}

#[tokio::test]
async fn redesign_pool_unhealthy_is_not_reported_as_empty() {
    let state = test_app_state(test_config(test_store()));
    let app = router_from_state(state);
    let (status, _) = get_json(&app, "/transactions/pool").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn redesign_committed_detail_requires_node_transaction_bytes() {
    let store = test_store();
    seed_genesis_baseline(&store);
    let hash = [0x37; 32];
    let mut batch = StoreBatch::new(&store);
    batch.put_tx_hash_map(&hash, 10, 1);
    batch.put_tx_index(
        10,
        1,
        &TxIndexEntry {
            tx_hash: hash,
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 1,
            outputs_count: 1,
            fee: 100,
            tx_size: 100,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.put_block_header(
        10,
        &CachedBlockHeader {
            hash: vec![0x41; 32],
            parent_hash: vec![0x42; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 2,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: 1_700_000_000_000,
        },
    );
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let (status, body) = get_json(
        &app,
        &format!("/transactions/0x{}/detail", hex::encode(hash)),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert!(body["message"].as_str().unwrap().contains("CKB RocksDB"));
}

#[tokio::test]
async fn redesign_transaction_list_value_is_positive_participant_sum() {
    let store = test_store();
    let tx = [0x61; 32];
    seed_committed_activity(&store, &[0x11; 32], &tx, 100, 1, -850, 0);
    let mut actions = store.get_tx_actions(100, 1, &tx).unwrap().unwrap();
    actions
        .participants
        .push(make_test_participant(0x12, 700, 0));
    actions
        .participants
        .push(make_test_participant(0x13, 50, 0));
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_tx_actions(&actions);
    batch.put_cell(
        &tx,
        0,
        &LiveCellInfo {
            capacity: 9900000000,
            lock_script_hash: vec![0x12; 32],
            lock_code_hash: vec![0x21; 32],
            lock_hash_type: 1,
            lock_args: vec![0; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 6100000000,
            udt_amount: None,
            data_hash: None,
        },
        100,
    );
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/transactions?limit=1").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["data"][0]["value"], "750");
    assert_eq!(json["data"][0]["outputCapacity"], "9900000000");
    assert_eq!(json["data"][0]["txSize"], 226);
    assert_eq!(
        json["data"][0]["activity"]["participants"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[tokio::test]
async fn redesign_empty_healthy_pool_has_exact_zero_summary_and_no_rates() {
    let state = test_app_state(test_config(test_store()));
    state.pool_mirror.publish(healthy_pool_snapshot(vec![]));
    let (status, body) = get_json(&router_from_state(state), "/transactions/pool").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], 0);
    assert_eq!(
        body["summary"],
        serde_json::json!({"size":"0","cycles":"0","minFeeRate":null,"maxFeeRate":null,"newestSeenAt":null})
    );
}

#[tokio::test]
async fn transaction_class_filter_pages_across_sparse_action_chunks() {
    use ckbadger_store::types::{TAG_TOKEN, TAG_TYPE_CALL};
    let store = test_store();
    let mut first = [0x45; 32];
    first[..4].copy_from_slice(&1i32.to_be_bytes());
    seed_committed_activity(&store, &[0x21; 32], &first, 100, 1, 1000, TAG_TOKEN);
    let (_, _, template) = store.get_tx_by_hash(&first).unwrap().unwrap();
    let mut batch = StoreBatch::new(store.as_ref());
    let mut header = store.get_block_header(100).unwrap().unwrap();
    header.transactions_count = 261;
    batch.put_block_header(100, &header);
    let cell = LiveCellInfo {
        capacity: 10_000_000_000,
        lock_script_hash: vec![0x21; 32],
        lock_code_hash: vec![0x11; 32],
        lock_hash_type: 1,
        lock_args: vec![0; 20],
        type_script_hash: None,
        type_code_hash: None,
        type_hash_type: None,
        type_args: None,
        data_size: 0,
        occupied_capacity: 6_100_000_000,
        udt_amount: None,
        data_hash: None,
    };
    for index in 1i32..=260 {
        let mut hash = [0x45; 32];
        hash[..4].copy_from_slice(&index.to_be_bytes());
        let tags = if [1, 130, 260].contains(&index) {
            TAG_TOKEN
        } else {
            TAG_TYPE_CALL
        };
        batch.put_tx_hash_map(&hash, 100, index);
        batch.put_tx_index(
            100,
            index,
            &TxIndexEntry {
                tx_hash: hash,
                ..template.clone()
            },
        );
        batch.put_tx_actions(&make_test_tx_actions(
            &[0x21; 32],
            &hash,
            &[0xba; 32],
            100,
            index,
            1000,
            tags,
        ));
        batch.put_cell(&hash, 0, &cell, 100);
    }
    let cellbase_hash = [0xcb; 32];
    // Production deliberately omits cellbase from CF_TX_ACTIONS.
    batch.put_tx_hash_map(&cellbase_hash, 100, 0);
    batch.put_tx_index(
        100,
        0,
        &TxIndexEntry {
            tx_hash: cellbase_hash,
            is_cellbase: true,
            cycles: None,
            ..template
        },
    );
    batch.put_cell(&cellbase_hash, 0, &cell, 100);
    let older_hash = [0xca; 32];
    let older_header = CachedBlockHeader {
        hash: vec![0xb9; 32],
        ..store.get_block_header(100).unwrap().unwrap()
    };
    batch.put_block_header(99, &older_header);
    batch.put_tx_hash_map(&older_hash, 99, 0);
    batch.put_tx_index(
        99,
        0,
        &TxIndexEntry {
            tx_hash: older_hash,
            is_cellbase: true,
            timestamp: 1_700_000_000_099,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 200,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.put_cell(&older_hash, 0, &cell, 99);
    batch.commit().unwrap();
    assert!(store
        .get_tx_actions(100, 0, &cellbase_hash)
        .unwrap()
        .is_none());
    let app = create_router(test_config(store)).await;
    let mut cursor = None;
    for expected in [260, 130, 1] {
        let url = format!(
            "/transactions?activity_type=token&limit=1{}",
            cursor
                .as_ref()
                .map(|c| format!("&cursor={c}"))
                .unwrap_or_default()
        );
        let (status, body) = get_json(&app, &url).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["data"][0]["index"], expected, "{body}");
        assert!(
            body.get("total").is_none(),
            "filtered total must not pretend to be all transactions"
        );
        cursor = body["nextCursor"].as_str().map(str::to_owned);
    }
    assert!(cursor.is_none());
    let (_, scripts) = get_json(&app, "/transactions?activity_type=script&limit=1").await;
    assert_eq!(
        scripts["data"][0]["index"], 259,
        "classification must use actions, not zero semantic_tags in TX_INDEX"
    );
    let (_, ckb) = get_json(&app, "/transactions?activity_type=ckb").await;
    assert!(ckb["data"].as_array().unwrap().is_empty());
    let (status, body) = get_json(&app, "/transactions?activity_type=cellbase&limit=1").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"].as_array().unwrap().len(), 1);
    assert_eq!(body["data"][0]["cycles"], 0);
    assert_eq!(body["data"][0]["cyclesStatus"], "complete");
    assert_eq!(body["data"][0]["blockNumber"], 100);
    assert_eq!(body["nextCursor"], "100:0");
    let (status, older) = get_json(
        &app,
        "/transactions?activity_type=cellbase&limit=1&cursor=100:0",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{older}");
    assert_eq!(older["data"][0]["blockNumber"], 99);
    assert!(older["nextCursor"].is_null());
    assert_eq!(
        get_json(&app, "/transactions?activity_type=nft").await.0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn pool_class_filter_precedes_paging_and_summary_and_excludes_partial() {
    use ckbadger_api::pool::{Interpretation, PoolStatus};
    use ckbadger_store::types::{TAG_TOKEN, TAG_TYPE_CALL};
    let state = test_app_state(test_config(test_store()));
    let mut partial =
        make_test_pool_record(&[0x14; 32], &[0x21; 32], 1000, 4000, PoolStatus::Pending);
    partial.interpretation = Interpretation::Partial {
        reasons: vec![ckbadger_api::pool::PartialReason::UnresolvedInput {
            tx_hash: [0xee; 32],
            index: 0,
        }],
    };
    partial.actions.as_mut().unwrap().participants[0].tags = TAG_TOKEN;
    state.pool_mirror.publish(healthy_pool_snapshot(vec![
        make_test_pool_record_with(
            &[0x11; 32],
            &[0x21; 32],
            1000,
            1000,
            PoolStatus::Pending,
            TAG_TOKEN,
            Interpretation::Complete,
        ),
        make_test_pool_record_with(
            &[0x12; 32],
            &[0x21; 32],
            1000,
            2000,
            PoolStatus::Proposed,
            TAG_TYPE_CALL,
            Interpretation::Complete,
        ),
        make_test_pool_record_with(
            &[0x13; 32],
            &[0x21; 32],
            1000,
            3000,
            PoolStatus::Pending,
            TAG_TOKEN,
            Interpretation::Complete,
        ),
        partial,
    ]));
    let app = router_from_state(state);
    let (status, body) = get_json(&app, "/transactions/pool?activity_type=token&limit=1").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], 2);
    assert_eq!(body["data"][0]["hash"], format!("0x{}", "13".repeat(32)));
    assert_eq!(body["summary"]["size"], "1000");
    assert_eq!(body["summary"]["cycles"], "400000");
    assert_eq!(body["summary"]["newestSeenAt"], "3000");
    let cursor = body["nextCursor"].as_str().unwrap();
    let (_, second) = get_json(
        &app,
        &format!("/transactions/pool?activity_type=token&limit=1&cursor={cursor}"),
    )
    .await;
    assert_eq!(second["summary"], body["summary"]);
    assert_eq!(second["data"][0]["hash"], format!("0x{}", "11".repeat(32)));
    assert!(second["nextCursor"].is_null());
    let (_, scripts) = get_json(&app, "/transactions/pool?activity_type=script").await;
    assert_eq!(scripts["total"], 1);
    assert_eq!(scripts["summary"]["size"], "500");
    let (_, all) = get_json(&app, "/transactions/pool").await;
    assert_eq!(all["total"], 4);
    assert_eq!(
        get_json(&app, "/transactions/pool?activity_type=nft")
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn scalar_cycles_do_not_complete_the_group_calculation_contract() {
    let store = test_store();
    let hash = [0x66; 32];
    insert_committed_transaction(&store, &hash);
    let requests = tempfile::tempdir().unwrap();
    let mut config = test_config(store.clone());
    config.cycles_request_dir = Some(requests.path().to_path_buf());
    let app = create_router(config).await;
    let (status, body) = get_json(
        &app,
        &format!("/transactions/0x{}/cycles", hex::encode(hash)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["status"], "queued",
        "a scalar-only total must request its canonical group result"
    );
    assert!(requests
        .path()
        .join(format!("tx_0x{}", hex::encode(hash)))
        .exists());
    assert!(body["cycles"].is_null(), "{body}");
    store
        .update_tx_cycles_with_groups(
            321,
            0,
            &hash,
            &ckbadger_common::cycles_task::TransactionCycles {
                block_hash: [0x44; 32],
                cycles: 333,
                groups: vec![ckbadger_common::cycles_task::ScriptGroupCycles {
                    script_hash: [0x21; 32],
                    group_type: ckbadger_common::cycles_task::ScriptGroupKind::Lock,
                    cycles: 333,
                }],
            },
        )
        .unwrap();
    let (status, body) = get_json(
        &app,
        &format!("/transactions/0x{}/cycles", hex::encode(hash)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "done");
    assert_eq!(body["cycles"], 333);
}

#[tokio::test]
async fn genesis_non_cellbase_cycles_are_zero_without_a_vm_request() {
    let store = test_store();
    let hash = [0x67; 32];
    insert_committed_transaction(&store, &hash);
    let (_, _, entry) = store.get_tx_by_hash(&hash).unwrap().unwrap();
    let header = store.get_block_header(321).unwrap().unwrap();
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(0, &header);
    batch.put_tx_hash_map(&hash, 0, 1);
    batch.put_tx_index(
        0,
        1,
        &TxIndexEntry {
            cycles: None,
            ..entry
        },
    );
    batch.commit().unwrap();
    let requests = tempfile::tempdir().unwrap();
    let mut config = test_config(store.clone());
    config.cycles_request_dir = Some(requests.path().to_path_buf());
    let app = create_router(config).await;
    let (status, body) = get_json(
        &app,
        &format!("/transactions/0x{}/cycles", hex::encode(hash)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "done");
    assert_eq!(body["cycles"], 0);
    assert_eq!(std::fs::read_dir(requests.path()).unwrap().count(), 0);
    store.update_tx_cycles(0, 1, &hash, 5).unwrap();
    let (status, body) = get_json(
        &app,
        &format!("/transactions/0x{}/cycles", hex::encode(hash)),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert!(body["message"]
        .as_str()
        .unwrap()
        .contains(&hex::encode(hash)));
    assert_eq!(store.get_tx_index(0, 1).unwrap().unwrap().cycles, Some(5));
}

#[tokio::test]
async fn genesis_detail_marks_type_groups_as_unexecuted() {
    use ckb_types::{
        bytes::Bytes,
        core::{BlockBuilder, EpochNumberWithFraction, TransactionBuilder},
        packed,
        prelude::*,
    };
    let store = test_store();
    seed_genesis_baseline(&store);
    let lock = packed::Script::new_builder()
        .code_hash(packed::Byte32::new([0x11; 32]))
        .hash_type(1u8.into())
        .args(Bytes::from(vec![0x22; 20]).pack())
        .build();
    let type_script = packed::Script::new_builder()
        .code_hash(packed::Byte32::new([0x33; 32]))
        .hash_type(1u8.into())
        .build();
    let tx = TransactionBuilder::default()
        .output(
            packed::CellOutput::new_builder()
                .capacity(12_300_000_000u64.pack())
                .lock(lock.clone())
                .type_(Some(type_script).pack())
                .build(),
        )
        .output_data(Bytes::new().pack())
        .build();
    assert!(!tx.is_cellbase());
    let hash: [u8; 32] = tx.hash().unpack();
    let block = BlockBuilder::default()
        .number(0u64.pack())
        .epoch(EpochNumberWithFraction::new(0, 0, 1000).pack())
        .transaction(
            TransactionBuilder::default()
                .input(packed::CellInput::new_cellbase_input(0))
                .witness(
                    packed::CellbaseWitness::new_builder()
                        .lock(lock.clone())
                        .build()
                        .as_bytes()
                        .pack(),
                )
                .build(),
        )
        .transaction(tx)
        .build();
    let block_hash: [u8; 32] = block.hash().unpack();
    let chain = seed_ckb_chain(&[block]);
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        0,
        &CachedBlockHeader {
            hash: block_hash.to_vec(),
            parent_hash: vec![0; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1000,
            dao: vec![0; 32],
            transactions_count: 2,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: 1_700_000_000_000,
        },
    );
    batch.put_tx_hash_map(&hash, 0, 1);
    batch.put_tx_index(
        0,
        1,
        &TxIndexEntry {
            tx_hash: hash,
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 0,
            outputs_count: 1,
            fee: 0,
            tx_size: 200,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.put_live_cell_marker_by_outpoint(&hash, 0, 0);
    batch.commit().unwrap();
    let app = create_router(test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    ))
    .await;
    let (status, body) = get_json(
        &app,
        &format!("/transactions/0x{}/detail", hex::encode(hash)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["cycles"], 0);
    assert_eq!(body["cyclesStatus"], "complete");
    let groups = body["scriptGroups"].as_array().unwrap();
    assert!(groups.iter().any(|group| group["kind"] == "type"));
    assert!(groups.iter().all(|group| group["ran"] == false
        && group["cycles"].is_null()
        && group["witnessIndex"].is_null()));
}
