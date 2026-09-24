mod common;
use common::*;

#[tokio::test]
async fn test_address_activities_reads_from_store() {
    let (core_store, append_only_store) = split_test_stores();
    let lock_hash = vec![0x22; 32];
    let tx_hash = vec![0xaa; 32];
    let block_hash = vec![0xba; 32];

    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_tx_hash_map(&tx_hash, 10, 0);
    core_batch.put_tx_index(
        10,
        0,
        &TxIndexEntry {
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
    core_batch.put_block_header(
        10,
        &CachedBlockHeader {
            hash: block_hash.clone(),
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
        },
    );
    let actions = make_test_tx_actions(&lock_hash, &tx_hash, &block_hash, 10, 0, 100, 0);
    core_batch.put_tx_actions(&actions);
    core_batch.put_addr_tx(
        &lock_hash,
        10,
        0,
        &tx_hash,
        &AddrTxValue::new(0, false, true, 0),
    );
    core_batch.commit().unwrap();
    core_store
        .update_sync_status(|s| {
            s.tip_block_number = 10;
        })
        .unwrap();

    let config = test_config_with_append_only(core_store.clone(), append_only_store.clone());
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/addresses/0x{}/activities",
            hex::encode(&lock_hash)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn test_address_activities_returns_protocol_metadata() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();
    let lock_hash = vec![0x24; 32];
    let tx_hash = vec![0xaa; 32];
    let block_hash = vec![0xbb; 32];

    let mut actions = make_test_tx_actions(&lock_hash, &tx_hash, &block_hash, 88, 1, 100, 0);
    actions.protocol_actions = vec![ProtocolAction::new(
        "stablepp",
        "deposit",
        serde_json::json!({
            "hasIntent": true,
            "vaultCount": 2,
        }),
    )];

    let mut batch = StoreBatch::new(core_store.as_ref());
    batch.put_tx_hash_map(&tx_hash, 88, 1);
    batch.put_tx_index(
        88,
        1,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_123,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 100,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.put_block_header(
        88,
        &CachedBlockHeader {
            hash: block_hash,
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_123,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
        },
    );
    batch.put_tx_actions(&actions);
    batch.put_addr_tx(
        &lock_hash,
        88,
        1,
        &tx_hash,
        &AddrTxValue::new(0, false, true, 0),
    );
    batch.commit().unwrap();
    core_store
        .update_sync_status(|s| {
            s.tip_block_number = 88;
        })
        .unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/addresses/0x{}/activities",
            hex::encode(&lock_hash)
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(
        json["data"][0]["protocolActions"][0]["protocol"],
        "stablepp"
    );
    assert_eq!(
        json["data"][0]["protocolActions"][0]["metadata"]["hasIntent"],
        true
    );
    assert_eq!(
        json["data"][0]["protocolActions"][0]["metadata"]["vaultCount"],
        2
    );
}

#[tokio::test]
async fn test_address_activities_rejects_unknown_filter() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();
    core_store
        .update_sync_status(|s| {
            s.tip_block_number = 10;
        })
        .unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/addresses/0x{}/activities?filter=tok",
            "11".repeat(32)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "bad_request");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("invalid activity filter"));
}

#[tokio::test]
async fn test_address_activities_return_type_calls_and_support_type_call_filter() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();
    let lock_hash = vec![0x12; 32];
    let tx_hash = vec![0x34; 32];
    let block_hash = vec![0x56; 32];
    let type_code_hash = vec![0x78; 32];
    let type_args = vec![0x9A; 20];
    let expected_script_hash = format!(
        "0x{}",
        hex::encode(compute_script_hash(&type_code_hash, 1, &type_args))
    );

    use ckbadger_store::types::{ParticipantDelta, ParticipantId, TAG_TYPE_CALL};
    let actions = TxActions {
        tx_hash: tx_hash.clone(),
        block_hash: block_hash.clone(),
        block_number: 88,
        tx_index: 0,
        timestamp: 1_700_000_888,
        is_cellbase: false,
        protocol_actions: vec![],
        type_calls: vec![TypeCallEntry {
            type_code_hash: type_code_hash.clone(),
            type_hash_type: 1,
            type_args: type_args.clone(),
        }],
        lock_calls: vec![],
        participants: vec![ParticipantDelta {
            id: ParticipantId::lock(&lock_hash).unwrap(),
            ckb_delta: 0,
            used_delta: 0,
            item_deltas: vec![],
            tags: TAG_TYPE_CALL,
            roles: 0,
        }],
    };

    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_tx_actions(&actions);
    // AddrTxValue.tags must mirror the participant's tags so filtered scans
    // hit the entry (list_activities pre-filters on AddrTxValue.tags).
    core_batch.put_addr_tx(
        &lock_hash,
        88,
        0,
        &tx_hash,
        &AddrTxValue::new(0, false, true, TAG_TYPE_CALL),
    );
    core_batch.put_tx_hash_map(&tx_hash, 88, 0);
    core_batch.put_tx_index(
        88,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_888_000,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 120,
            cycles: None,
            semantic_tags: 0,
        },
    );
    core_batch.put_block_header(
        88,
        &CachedBlockHeader {
            hash: block_hash,
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_888_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
        },
    );
    core_batch.put_script_info(
        &type_code_hash,
        &ScriptInfo {
            code_hash: type_code_hash.clone(),
            hash_type: 1,
            name: Some("RGB++ Lock".to_string()),
            ..Default::default()
        },
    );
    core_batch.commit().unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/addresses/0x{}/activities?filter=type_call",
            hex::encode(&lock_hash)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["itemDeltas"].as_array().unwrap().len(), 0);
    let type_calls = data[0]["typeCalls"].as_array().unwrap();
    assert_eq!(type_calls.len(), 1);
    assert_eq!(
        type_calls[0]["typeCodeHash"],
        format!("0x{}", hex::encode(&type_code_hash))
    );
    assert_eq!(type_calls[0]["typeHashType"], "type");
    assert_eq!(
        type_calls[0]["typeArgs"],
        format!("0x{}", hex::encode(&type_args))
    );
    assert_eq!(type_calls[0]["scriptHash"], expected_script_hash);
    assert_eq!(type_calls[0]["scriptName"], "RGB++ Lock");
    let lock_calls = data[0]["lockCalls"].as_array().unwrap();
    assert_eq!(lock_calls.len(), 0);
}

#[tokio::test]
async fn test_latest_activities_return_type_calls() {
    use ckbadger_store::types::{ParticipantDelta, ParticipantId, TAG_DAO, TAG_TYPE_CALL};
    let core_store = test_store();
    let append_only_store = test_append_only_store();
    let tx_hash = vec![0x68; 32];
    let block_hash = vec![0x79; 32];
    let type_code_hash = vec![0x46; 32];
    let type_args = vec![0x57; 20];
    let expected_script_hash = format!(
        "0x{}",
        hex::encode(compute_script_hash(&type_code_hash, 1, &type_args))
    );

    let actions = TxActions {
        tx_hash: tx_hash.clone(),
        block_hash: block_hash.clone(),
        block_number: 99,
        tx_index: 1,
        timestamp: 1_700_000_999,
        is_cellbase: false,
        protocol_actions: vec![ProtocolAction::new(
            "dao",
            "deposit",
            serde_json::json!({"capacity": 102_00000000i64}),
        )],
        type_calls: vec![TypeCallEntry {
            type_code_hash: type_code_hash.clone(),
            type_hash_type: 1,
            type_args: type_args.clone(),
        }],
        lock_calls: vec![],
        participants: vec![ParticipantDelta {
            id: ParticipantId::Lock([0x13; 32]),
            ckb_delta: -30000,
            used_delta: 0,
            item_deltas: vec![],
            tags: TAG_TYPE_CALL | TAG_DAO,
            roles: 0,
        }],
    };

    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_script_info(
        &type_code_hash,
        &ScriptInfo {
            code_hash: type_code_hash.clone(),
            hash_type: 1,
            name: Some("RGB++ Lock".to_string()),
            ..Default::default()
        },
    );
    core_batch.put_block_header(
        99,
        &CachedBlockHeader {
            hash: block_hash,
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_999,
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
        },
    );
    core_batch.put_tx_hash_map(&tx_hash, 99, 1);
    core_batch.put_tx_index(
        99,
        1,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_999,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 1,
            cycles: None,
            semantic_tags: 0,
        },
    );
    core_batch.put_tx_actions(&actions);
    core_batch.commit().unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri("/api/v1/activities/latest?limit=1")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let items = json.as_array().unwrap();
    assert_eq!(items.len(), 1);
    let type_calls = items[0]["typeCalls"].as_array().unwrap();
    assert_eq!(type_calls.len(), 1);
    assert_eq!(type_calls[0]["typeHashType"], "type");
    assert_eq!(type_calls[0]["scriptHash"], expected_script_hash);
    assert_eq!(type_calls[0]["scriptName"], "RGB++ Lock");
    let lock_calls = items[0]["lockCalls"].as_array().unwrap();
    assert_eq!(lock_calls.len(), 0);
}

// Global activities cursor pagination test removed: owner-level pagination replaced with TX-level
// in the TxActions model. The /api/v1/activities endpoint now returns TX-level items.

#[tokio::test]
async fn test_global_activities_basic() {
    let store = test_store();
    let tx_hash = vec![0x91; 32];
    let block_hash = vec![0xA1; 32];

    let actions = make_test_tx_actions(&[0x11; 32], &tx_hash, &block_hash, 200, 0, 111, 0);

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        200,
        &CachedBlockHeader {
            hash: block_hash,
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_200,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
        },
    );
    batch.put_tx_hash_map(&tx_hash, 200, 0);
    batch.put_tx_index(
        200,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_200,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 100,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.put_tx_actions(&actions);
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;

    let request = Request::builder()
        .uri("/api/v1/activities?limit=10")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().expect("data array");
    assert!(!data.is_empty());
    assert_eq!(data[0]["txHash"], format!("0x{}", hex::encode(&tx_hash)));
}

#[tokio::test]
async fn test_address_transactions_reads_from_derived_store() {
    let (core_store, append_only_store) = split_test_stores();
    let lock_hash = vec![0x33; 32];
    let tx_hash = vec![0xab; 32];

    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_tx_hash_map(&tx_hash, 10, 0);
    core_batch.put_tx_index(
        10,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 1,
            outputs_count: 2,
            fee: 1000,
            tx_size: 120,
            cycles: Some(10_000),
            semantic_tags: 0,
        },
    );
    // A canonical transaction's block has a header; the list reads its time.
    core_batch.put_block_header(
        10,
        &CachedBlockHeader {
            hash: vec![0xba; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
        },
    );
    core_batch.commit().unwrap();
    core_store
        .update_sync_status(|s| {
            s.tip_block_number = 10;
        })
        .unwrap();

    let config = test_config_with_append_only(core_store.clone(), append_only_store.clone());
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/addresses/0x{}/transactions",
            hex::encode(&lock_hash)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 0);

    let mut derived_batch = StoreBatch::new(append_only_store.as_ref());
    derived_batch.put_addr_tx(
        &lock_hash,
        10,
        0,
        &tx_hash,
        &AddrTxValue::new(0, false, true, 0),
    );
    derived_batch.commit().unwrap();

    let request = Request::builder()
        .uri(format!(
            "/api/v1/addresses/0x{}/transactions",
            hex::encode(&lock_hash)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["txHash"], format!("0x{}", hex::encode(&tx_hash)));
}

// ---------------------------------------------------------------------------
// R4-G item 3: `/addresses/{addr}/activities` declared `total` from
// `AddressBalance.txs_count` — the address's TRANSACTION count. Activities are
// a different set: cellbase rows are deliberately never persisted in
// CF_TX_ACTIONS, and `is_canonical_activity` drops more. Measured on mainnet
// (block 12000000's cellbase-output address): declared total 4,727,769 against
// 1,682 rows enumerated to exhaustion — a total the endpoint can never reach.
// No per-address activity count is stored, so the honest contract is no total.
// ---------------------------------------------------------------------------

/// Seed one canonical activity for `lock_hash` plus an `AddressBalance` whose
/// `txs_count` deliberately disagrees with it (the shape a miner address has:
/// many cellbase transactions, few activities).
fn seed_activity_with_txs_count(
    store: &Arc<CkbadgerStore>,
    lock_hash: &[u8],
    txs_count: i64,
) -> Vec<u8> {
    let tx_hash = vec![0xa1; 32];
    let block_hash = vec![0xb1; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_tx_hash_map(&tx_hash, 10, 0);
    batch.put_tx_index(
        10,
        0,
        &TxIndexEntry {
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
    batch.put_block_header(
        10,
        &CachedBlockHeader {
            hash: block_hash.clone(),
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
        },
    );
    let actions = make_test_tx_actions(lock_hash, &tx_hash, &block_hash, 10, 0, 100, 0);
    batch.put_tx_actions(&actions);
    batch.put_addr_tx(
        lock_hash,
        10,
        0,
        &tx_hash,
        &AddrTxValue::new(0, false, true, 0),
    );
    batch.commit().unwrap();

    store
        .put_addr_balance_direct(
            lock_hash,
            &ckbadger_store::types::AddressBalance {
                balance: 100,
                used_capacity: 0,
                live_cells_count: 1,
                total_cells_count: 1,
                txs_count,
                first_seen_block: 10,
                first_seen_tx: tx_hash.clone(),
                last_activity_block: 10,
                last_activity_tx: tx_hash.clone(),
            },
        )
        .unwrap();
    store
        .update_sync_status(|s| {
            s.tip_block_number = 10;
        })
        .unwrap();

    tx_hash
}

#[tokio::test]
async fn test_address_activities_declares_no_unreachable_total() {
    let (core_store, append_only_store) = split_test_stores();
    let lock_hash = vec![0x71; 32];
    // 4_727_769 transactions, exactly 1 of which is an activity.
    seed_activity_with_txs_count(&core_store, &lock_hash, 4_727_769);

    let config = test_config_with_append_only(core_store.clone(), append_only_store);
    let app = create_router(config).await;
    let (status, json) = get_json(
        &app,
        &format!("/addresses/0x{}/activities", hex::encode(&lock_hash)),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["hasMore"], false);
    assert_eq!(
        json.get("total"),
        None,
        "the endpoint enumerates 1 activity; it must not declare a transaction count it can never reach, got {json}"
    );
}

#[tokio::test]
async fn test_address_activities_omits_total_for_missing_addr_balance() {
    // An address with activities but no `AddressBalance` row used to report
    // `total: 0` through `.ok().flatten().map(...).unwrap_or(0)` — a silent
    // default-zero on a correctness path, and self-contradictory next to a
    // non-empty page.
    let (core_store, append_only_store) = split_test_stores();
    let lock_hash = vec![0x72; 32];
    let tx_hash = vec![0xa1; 32];
    let block_hash = vec![0xb1; 32];

    let mut batch = StoreBatch::new(core_store.as_ref());
    batch.put_tx_hash_map(&tx_hash, 10, 0);
    batch.put_tx_index(
        10,
        0,
        &TxIndexEntry {
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
    batch.put_block_header(
        10,
        &CachedBlockHeader {
            hash: block_hash.clone(),
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
        },
    );
    let actions = make_test_tx_actions(&lock_hash, &tx_hash, &block_hash, 10, 0, 100, 0);
    batch.put_tx_actions(&actions);
    batch.put_addr_tx(
        &lock_hash,
        10,
        0,
        &tx_hash,
        &AddrTxValue::new(0, false, true, 0),
    );
    batch.commit().unwrap();
    core_store
        .update_sync_status(|s| {
            s.tip_block_number = 10;
        })
        .unwrap();

    let config = test_config_with_append_only(core_store.clone(), append_only_store);
    let app = create_router(config).await;
    let (status, json) = get_json(
        &app,
        &format!("/addresses/0x{}/activities", hex::encode(&lock_hash)),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(
        json.get("total"),
        None,
        "a missing balance row must not be rendered as `total: 0` next to a non-empty page, got {json}"
    );
}

#[tokio::test]
async fn test_address_activities_filtered_page_still_omits_total() {
    // Control (passes on both revisions): the filtered branch already used
    // `without_total`. Both branches now agree.
    let (core_store, append_only_store) = split_test_stores();
    let lock_hash = vec![0x73; 32];
    seed_activity_with_txs_count(&core_store, &lock_hash, 4_727_769);

    let config = test_config_with_append_only(core_store.clone(), append_only_store);
    let app = create_router(config).await;
    let (status, json) = get_json(
        &app,
        &format!(
            "/addresses/0x{}/activities?filter=ckb",
            hex::encode(&lock_hash)
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json.get("total"), None);
}

// ---------------------------------------------------------------------------
// Audited bug (2026-08-01 night, agent E): all-uppercase bech32m addresses are
// legal per the bech32 case rules, but every address entry point routed them
// into the hex-hash branch (the activities handler even had its own inline
// lowercase-only prefix check) and answered a misleading 400 about hex hashes.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_address_activities_accepts_uppercase_address() {
    let (core_store, append_only_store) = split_test_stores();

    // Mainnet burn lock (secp sighash, args = 20 zero bytes); its canonical
    // bech32m encoding is the audit vector below.
    let burn_address = "ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqgqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq5m759c";
    let code_hash =
        hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8").unwrap();
    let lock_hash = compute_script_hash(&code_hash, 1, &[0u8; 20]);
    seed_activity_with_txs_count(&core_store, &lock_hash, 1);

    let config = test_config_with_append_only(core_store.clone(), append_only_store);
    let app = create_router(config).await;

    let (status_lower, lower) =
        get_json(&app, &format!("/addresses/{burn_address}/activities")).await;
    assert_eq!(status_lower, StatusCode::OK, "got {lower}");
    assert_eq!(lower["data"].as_array().unwrap().len(), 1);

    let (status_upper, upper) = get_json(
        &app,
        &format!("/addresses/{}/activities", burn_address.to_uppercase()),
    )
    .await;
    assert_eq!(
        status_upper,
        StatusCode::OK,
        "uppercase bech32m must route to the address branch, got {upper}"
    );
    assert_eq!(
        upper, lower,
        "uppercase input must enumerate the identical activities"
    );
}

// ---------------------------------------------------------------------------
// Tx-pool rows on page one
//
// A pool transaction can only land in a FUTURE block, so it is later than every
// committed transaction in canonical order. That makes pool rows a page-one-only
// segment: above the committed rows, never inside a cursor.
// ---------------------------------------------------------------------------

const POOL_LOCK_HASH: [u8; 32] = [0x41; 32];

#[tokio::test]
async fn test_address_activities_page_one_puts_pool_rows_above_committed_rows() {
    let store = test_store();
    seed_committed_activity(&store, &POOL_LOCK_HASH, &[0xc1; 32], 10, 0, 100, 0);

    let state = test_app_state(test_config(store));
    state
        .pool_mirror
        .publish(healthy_pool_snapshot(vec![make_test_pool_record(
            &[0xf1; 32],
            &POOL_LOCK_HASH,
            -500,
            1_700_000_500_000,
            ckbadger_api::pool::PoolStatus::Pending,
        )]));
    let app = create_router_with_state(state).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/addresses/0x{}/activities",
                    hex::encode(POOL_LOCK_HASH)
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "pool row then committed row: {json:?}");

    let pool_row = &rows[0];
    assert_eq!(pool_row["txHash"], format!("0x{}", "f1".repeat(32)));
    assert_eq!(pool_row["blockNumber"], serde_json::Value::Null);
    assert_eq!(pool_row["txIndex"], serde_json::Value::Null);
    assert_eq!(pool_row["timestamp"], serde_json::Value::Null);
    assert_eq!(pool_row["poolStatus"], "pending");
    assert!(pool_row["timeAddedToPool"].as_str().is_some());
    assert_eq!(pool_row["interpretation"]["status"], "complete");
    assert_eq!(pool_row["ckbDelta"], "-500");

    let committed_row = &rows[1];
    assert_eq!(committed_row["blockNumber"], 10);
    assert_eq!(committed_row["poolStatus"], serde_json::Value::Null);

    let pool = &json["pool"];
    assert_eq!(pool["enabled"], true);
    assert_eq!(pool["healthy"], true);
    assert_eq!(pool["count"], 1);
    assert_eq!(pool["pendingCkbDelta"], "-500");
    assert_eq!(pool["truncated"], false);
}

#[tokio::test]
async fn test_address_activities_cursor_page_contains_no_pool_rows() {
    let store = test_store();
    seed_committed_activity(&store, &POOL_LOCK_HASH, &[0xc1; 32], 10, 0, 100, 0);

    let state = test_app_state(test_config(store));
    state
        .pool_mirror
        .publish(healthy_pool_snapshot(vec![make_test_pool_record(
            &[0xf1; 32],
            &POOL_LOCK_HASH,
            -500,
            1_700_000_500_000,
            ckbadger_api::pool::PoolStatus::Pending,
        )]));
    let app = create_router_with_state(state).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/addresses/0x{}/activities?cursor=999:0",
                    hex::encode(POOL_LOCK_HASH)
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert!(
        rows.iter().all(|row| row["poolStatus"].is_null()),
        "cursor pages must stay stable while the pool churns: {json:?}"
    );
    assert!(
        json["pool"].is_null(),
        "the pool summary belongs to page one only"
    );
}

#[tokio::test]
async fn test_address_activities_filter_applies_to_pool_rows() {
    use ckbadger_store::types::TAG_TOKEN;

    let store = test_store();
    let state = test_app_state(test_config(store));
    state
        .pool_mirror
        .publish(healthy_pool_snapshot(vec![make_test_pool_record_with(
            &[0xf1; 32],
            &POOL_LOCK_HASH,
            -500,
            1_700_000_500_000,
            ckbadger_api::pool::PoolStatus::Pending,
            TAG_TOKEN,
            ckbadger_api::pool::Interpretation::Complete,
        )]));
    let app = create_router_with_state(state).await;

    for (filter, expected) in [("token", 1usize), ("dao", 0), ("all", 1)] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/addresses/0x{}/activities?filter={filter}",
                        hex::encode(POOL_LOCK_HASH)
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json["data"].as_array().unwrap().len(),
            expected,
            "filter={filter} must apply to pool rows through the same tag bitmap"
        );
        assert_eq!(json["pool"]["count"], expected);
    }
}

#[tokio::test]
async fn test_address_activities_omit_pool_rows_the_store_already_has() {
    let store = test_store();
    let tx_hash = [0xc1; 32];
    seed_committed_activity(&store, &POOL_LOCK_HASH, &tx_hash, 10, 0, 100, 0);

    let state = test_app_state(test_config(store));
    // The mirror still holds the record while it waits for the local index;
    // this request's store view already has the transaction.
    state
        .pool_mirror
        .publish(healthy_pool_snapshot(vec![make_test_pool_record(
            &tx_hash,
            &POOL_LOCK_HASH,
            100,
            1_700_000_500_000,
            ckbadger_api::pool::PoolStatus::CommittedAwaitingIndex {
                block_number: 10,
                block_hash: [0xba; 32],
            },
        )]));
    let app = create_router_with_state(state).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/addresses/0x{}/activities",
                    hex::encode(POOL_LOCK_HASH)
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "committed wins, exactly once: {json:?}");
    assert_eq!(rows[0]["blockNumber"], 10);
    assert_eq!(json["pool"]["count"], 0);
}

#[tokio::test]
async fn test_address_activities_report_an_unhealthy_mirror_and_still_serve_committed_rows() {
    let store = test_store();
    seed_committed_activity(&store, &POOL_LOCK_HASH, &[0xc1; 32], 10, 0, 100, 0);

    let state = test_app_state(test_config(store));
    state.pool_mirror.publish(unhealthy_pool_snapshot());
    let app = create_router_with_state(state).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/addresses/0x{}/activities",
                    hex::encode(POOL_LOCK_HASH)
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["pool"]["enabled"], true);
    assert_eq!(
        json["pool"]["healthy"], false,
        "an unreachable node must never read as an empty pool"
    );
}

#[tokio::test]
async fn test_address_activities_report_a_disabled_mirror() {
    let store = test_store();
    let mut config = test_config(store);
    config.pool_mirror_enabled = false;
    let app = create_router(config).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/addresses/0x{}/activities",
                    hex::encode(POOL_LOCK_HASH)
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["pool"]["enabled"], false);
    assert_eq!(json["pool"]["healthy"], false);
    assert_eq!(json["pool"]["count"], 0);
}

// ── Phase 1a: protocol-named participants ────────────────────────────────

/// Seed one block header + tx index + `TxActions` at `(block, 0)`.
fn seed_named_tx(
    store: &std::sync::Arc<CkbadgerStore>,
    block_number: i64,
    tx_hash: &[u8],
    actions: &ckbadger_store::types::TxActions,
) {
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_tx_hash_map(tx_hash, block_number, 0);
    batch.put_tx_index(
        block_number,
        0,
        &TxIndexEntry {
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
    batch.put_block_header(
        block_number,
        &CachedBlockHeader {
            hash: actions.block_hash.clone(),
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
        },
    );
    batch.put_tx_actions(actions);
    batch.commit().unwrap();
    store
        .update_sync_status(|s| {
            s.tip_block_number = s.tip_block_number.max(block_number);
        })
        .unwrap();
}

#[tokio::test]
async fn test_address_activities_include_named_participation_with_zero_ckb_delta() {
    use ckbadger_store::types::{
        participant_roles, AddrTxValue, ItemDelta, ItemKind, ParticipantDelta, ParticipantId,
        TAG_IDENTITY,
    };

    let core_store = test_store();
    let append_only_store = test_append_only_store();
    let lock_hash = vec![0x42u8; 32];
    let other = vec![0x11u8; 32];
    let tx_hash = vec![0xaa; 32];
    let block_hash = vec![0xbb; 32];
    let mut prefix = [0u8; 20];
    prefix.copy_from_slice(&lock_hash[..20]);

    let actions = ckbadger_store::types::TxActions {
        tx_hash: tx_hash.clone(),
        block_hash: block_hash.clone(),
        block_number: 10,
        tx_index: 0,
        timestamp: 1_700_000_000_000,
        is_cellbase: false,
        protocol_actions: vec![],
        type_calls: vec![],
        lock_calls: vec![],
        participants: vec![
            ParticipantDelta {
                id: ParticipantId::lock(&other).unwrap(),
                ckb_delta: -100,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
                roles: 0,
            },
            ParticipantDelta {
                id: ParticipantId::LockPrefix(prefix),
                ckb_delta: 0,
                used_delta: 0,
                item_deltas: vec![ItemDelta {
                    item_id: vec![0xEE; 20],
                    kind: ItemKind::Identity(IdentityStandard::DotCell),
                    magnitude: 1,
                    negative: false,
                }],
                tags: TAG_IDENTITY,
                roles: participant_roles::OWNER_TO,
            },
        ],
    };
    seed_named_tx(&core_store, 10, &tx_hash, &actions);
    let mut batch = StoreBatch::new(core_store.as_ref());
    batch.put_addr_tx_by_prefix(
        &prefix,
        10,
        0,
        &tx_hash,
        &AddrTxValue::new(0, false, false, TAG_IDENTITY),
    );
    // `other` holds a cell in this transaction, so the chain view knows its
    // lock script (CF_LOCK_SCRIPTS is written at every cell creation).
    batch.put_lock_script(
        &other,
        &ckbadger_store::types::LockScriptEntry {
            code_hash: hex::decode(
                "9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            )
            .unwrap(),
            hash_type: 1,
            args: vec![0x11; 20],
        },
    );
    batch.commit().unwrap();

    let config = test_config_with_append_only(core_store.clone(), append_only_store.clone());
    let app = create_router(config).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/addresses/0x{}/activities",
                    hex::encode(&lock_hash)
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{json:?}");
    assert_eq!(rows[0]["ckbDelta"], "0");
    assert_eq!(rows[0]["usedDelta"], "0");
    assert_eq!(rows[0]["itemDeltas"][0]["kind"], "identity");
    assert_eq!(rows[0]["itemDeltas"][0]["delta"], 1);
    assert_eq!(rows[0]["itemDeltas"][0]["standard"], "dotcell");
    assert_eq!(rows[0]["roles"][0], "owner_to");
    let participants = rows[0]["participants"].as_array().unwrap();
    assert_eq!(participants.len(), 1, "only the other party: {json:?}");
    assert_eq!(
        participants[0]["lockHash"],
        format!("0x{}", hex::encode(&other))
    );
    assert!(participants[0]["address"].is_string());
    assert!(participants[0]["lockHashPrefix"].is_null());
}

#[tokio::test]
async fn test_address_activities_other_participants_carry_prefix_and_resolution() {
    use ckbadger_store::types::{
        participant_roles, AddrTxValue, LockScriptEntry, ParticipantDelta, ParticipantId,
    };

    let core_store = test_store();
    let append_only_store = test_append_only_store();
    let lock_hash = vec![0x42u8; 32];
    let tx_hash = vec![0xaa; 32];
    let block_hash = vec![0xbb; 32];
    let named_prefix = [0x99u8; 20];

    let actions = ckbadger_store::types::TxActions {
        tx_hash: tx_hash.clone(),
        block_hash: block_hash.clone(),
        block_number: 10,
        tx_index: 0,
        timestamp: 1_700_000_000_000,
        is_cellbase: false,
        protocol_actions: vec![],
        type_calls: vec![],
        lock_calls: vec![],
        participants: vec![
            ParticipantDelta {
                id: ParticipantId::lock(&lock_hash).unwrap(),
                ckb_delta: -100,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
                roles: 0,
            },
            ParticipantDelta {
                id: ParticipantId::LockPrefix(named_prefix),
                ckb_delta: 0,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
                roles: participant_roles::OWNER_TO,
            },
        ],
    };
    seed_named_tx(&core_store, 10, &tx_hash, &actions);
    let mut batch = StoreBatch::new(core_store.as_ref());
    batch.put_addr_tx(
        &lock_hash,
        10,
        0,
        &tx_hash,
        &AddrTxValue::new(-100, true, false, 0),
    );
    batch.commit().unwrap();

    let uri = format!("/api/v1/addresses/0x{}/activities", hex::encode(&lock_hash));

    // Unresolved: no lock script starts with this prefix.
    let app = create_router(test_config_with_append_only(
        core_store.clone(),
        append_only_store.clone(),
    ))
    .await;
    let response = app
        .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let participant = &json["data"][0]["participants"][0];
    assert!(participant["address"].is_null(), "{json:?}");
    assert!(participant["lockHash"].is_null());
    assert_eq!(
        participant["lockHashPrefix"],
        format!("0x{}", hex::encode(named_prefix))
    );
    assert_eq!(participant["roles"][0], "owner_to");

    // Resolved: one lock script carries that prefix.
    let mut resolved_hash = [0x99u8; 32];
    resolved_hash[31] = 0x01;
    let mut batch = StoreBatch::new(core_store.as_ref());
    batch.put_lock_script(
        &resolved_hash,
        &LockScriptEntry {
            code_hash: hex::decode(
                "9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            )
            .unwrap(),
            hash_type: 1,
            args: vec![0x22; 20],
        },
    );
    batch.commit().unwrap();
    let app = create_router(test_config_with_append_only(
        core_store.clone(),
        append_only_store.clone(),
    ))
    .await;
    let response = app
        .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let participant = &json["data"][0]["participants"][0];
    assert_eq!(
        participant["lockHash"],
        format!("0x{}", hex::encode(resolved_hash))
    );
    assert!(participant["address"].as_str().unwrap().starts_with("ck"));

    // Ambiguous: a second lock script with the same prefix is an error, never a guess.
    let mut second_hash = [0x99u8; 32];
    second_hash[31] = 0x02;
    let mut batch = StoreBatch::new(core_store.as_ref());
    batch.put_lock_script(
        &second_hash,
        &LockScriptEntry {
            code_hash: hex::decode(
                "9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            )
            .unwrap(),
            hash_type: 1,
            args: vec![0x33; 20],
        },
    );
    batch.commit().unwrap();
    let app = create_router(test_config_with_append_only(
        core_store.clone(),
        append_only_store.clone(),
    ))
    .await;
    let response = app
        .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(
        String::from_utf8_lossy(&body).contains("ambiguous"),
        "{}",
        String::from_utf8_lossy(&body)
    );
}

#[tokio::test]
async fn test_global_activities_expose_participant_ids_and_roles() {
    use ckbadger_store::types::{participant_roles, ParticipantDelta, ParticipantId};

    let core_store = test_store();
    let append_only_store = test_append_only_store();
    let lock_hash = vec![0x42u8; 32];
    let tx_hash = vec![0xaa; 32];
    let block_hash = vec![0xbb; 32];
    let named_prefix = [0x99u8; 20];

    let actions = ckbadger_store::types::TxActions {
        tx_hash: tx_hash.clone(),
        block_hash: block_hash.clone(),
        block_number: 10,
        tx_index: 0,
        timestamp: 1_700_000_000_000,
        is_cellbase: false,
        protocol_actions: vec![],
        type_calls: vec![],
        lock_calls: vec![],
        participants: vec![
            ParticipantDelta {
                id: ParticipantId::lock(&lock_hash).unwrap(),
                ckb_delta: -100,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
                roles: 0,
            },
            ParticipantDelta {
                id: ParticipantId::LockPrefix(named_prefix),
                ckb_delta: 0,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
                roles: participant_roles::OWNER_TO | participant_roles::MANAGER_TO,
            },
        ],
    };
    seed_named_tx(&core_store, 10, &tx_hash, &actions);

    let app = create_router(test_config_with_append_only(
        core_store.clone(),
        append_only_store.clone(),
    ))
    .await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/activities")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let participants = json["data"][0]["participants"].as_array().unwrap();
    assert_eq!(participants.len(), 2, "{json:?}");
    assert_eq!(
        participants[0]["lockHash"],
        format!("0x{}", hex::encode(&lock_hash))
    );
    assert!(participants[0]["lockHashPrefix"].is_null());
    assert_eq!(participants[0]["roles"].as_array().unwrap().len(), 0);
    assert!(participants[1]["lockHash"].is_null());
    assert_eq!(
        participants[1]["lockHashPrefix"],
        format!("0x{}", hex::encode(named_prefix))
    );
    assert_eq!(participants[1]["roles"][0], "owner_to");
    assert_eq!(participants[1]["roles"][1], "manager_to");
}

// ── Task 2.8: timestamps propagate, one pool segment, pendingSummary ─────

async fn fetch_json(app: axum::Router, uri: &str) -> (StatusCode, serde_json::Value) {
    let response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
    (status, json)
}

/// The pool summary's `lastPolledAt` is the mirror's own clock. A value that
/// cannot be rendered is a mirror invariant violation, reported with the value
/// — not a `null` that reads as "never polled".
#[tokio::test]
async fn pool_summary_unrenderable_last_polled_at_is_a_500() {
    let store = test_store();
    let state = test_app_state(test_config(store));
    state
        .pool_mirror
        .publish(ckbadger_api::pool::PoolSnapshot::from_records(
            vec![Arc::new(make_test_pool_record(
                &[0xf1; 32],
                &POOL_LOCK_HASH,
                -500,
                1_700_000_500_000,
                ckbadger_api::pool::PoolStatus::Pending,
            ))],
            ckbadger_api::pool::MirrorStatus {
                enabled: true,
                healthy: true,
                last_polled_at_ms: Some(i64::MAX),
                ..Default::default()
            },
        ));
    let app = create_router_with_state(state).await;

    for uri in [
        format!(
            "/api/v1/addresses/0x{}/activities",
            hex::encode(POOL_LOCK_HASH)
        ),
        format!(
            "/api/v1/addresses/0x{}/transactions",
            hex::encode(POOL_LOCK_HASH)
        ),
    ] {
        let (status, json) = fetch_json(app.clone(), &uri).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{uri}: {json}");
        let message = json["message"].as_str().unwrap();
        assert!(message.contains("last_polled_at"), "{uri}: {message}");
        assert!(message.contains(&i64::MAX.to_string()), "{uri}: {message}");
    }
}

/// A canonical committed row whose block header is missing is store
/// corruption: the transaction list reports it with the block, instead of
/// serving the row with an empty timestamp.
#[tokio::test]
async fn address_transactions_missing_block_header_is_a_500_naming_the_block() {
    let store = test_store();
    let lock_hash = [0x4au8; 32];
    let tx_hash = [0x5bu8; 32];
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_tx_hash_map(&tx_hash, 4242, 0);
    batch.put_tx_index(
        4242,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 1,
            outputs_count: 1,
            fee: 100,
            tx_size: 300,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.put_addr_tx(
        &lock_hash,
        4242,
        0,
        &tx_hash,
        &AddrTxValue::new(-100, true, false, 0),
    );
    batch.commit().unwrap();
    let mut config = test_config(store);
    config.pool_mirror_enabled = false;
    let app = create_router(config).await;

    let (status, json) = fetch_json(
        app,
        &format!(
            "/api/v1/addresses/0x{}/transactions",
            hex::encode(lock_hash)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    let message = json["message"].as_str().unwrap();
    assert!(message.contains("4242"), "{message}");
    assert!(message.contains("header"), "{message}");
}

/// The address detail's `pendingSummary` counts every pool row of the address
/// — the same segment the lists serve on page one, committed-wins included —
/// and no activity filter reaches it, so the header cannot disagree with
/// itself between tabs.
#[tokio::test]
async fn address_pending_summary_is_independent_of_the_activity_filter() {
    use ckbadger_store::types::TAG_TOKEN;

    let store = test_store();
    let committed_tx = [0xc1; 32];
    seed_committed_activity(&store, &POOL_LOCK_HASH, &committed_tx, 10, 0, 100, 0);
    let state = test_app_state(test_config(store));
    state.pool_mirror.publish(healthy_pool_snapshot(vec![
        make_test_pool_record_with(
            &[0xf1; 32],
            &POOL_LOCK_HASH,
            -500,
            1_700_000_500_000,
            ckbadger_api::pool::PoolStatus::Pending,
            TAG_TOKEN,
            ckbadger_api::pool::Interpretation::Complete,
        ),
        make_test_pool_record(
            &[0xf2; 32],
            &POOL_LOCK_HASH,
            300,
            1_700_000_600_000,
            ckbadger_api::pool::PoolStatus::Proposed,
        ),
        // Already indexed: served by the committed segment, counted nowhere here.
        make_test_pool_record(
            &committed_tx,
            &POOL_LOCK_HASH,
            100,
            1_700_000_400_000,
            ckbadger_api::pool::PoolStatus::CommittedAwaitingIndex {
                block_number: 10,
                block_hash: [0xba; 32],
            },
        ),
    ]));
    let app = create_router_with_state(state).await;
    let addr = format!("/api/v1/addresses/0x{}", hex::encode(POOL_LOCK_HASH));

    let (status, detail) = fetch_json(app.clone(), &addr).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(
        detail["pendingSummary"],
        serde_json::json!({ "txCount": 2, "capacityDelta": "-200" })
    );

    // A filter on a list narrows the list's own `pool` summary …
    let (_, filtered) = fetch_json(app.clone(), &format!("{addr}/activities?filter=token")).await;
    assert_eq!(filtered["pool"]["count"], 1);
    assert_eq!(filtered["pool"]["pendingCkbDelta"], "-500");
    // … the transaction list agrees with the unfiltered segment …
    let (_, txs) = fetch_json(app.clone(), &format!("{addr}/transactions")).await;
    assert_eq!(txs["pool"]["count"], 2);
    assert_eq!(txs["pool"]["pendingCkbDelta"], "-200");
    // … and the detail's summary is the same whatever the page asks for.
    let (_, again) = fetch_json(app, &format!("{addr}?filter=token")).await;
    assert_eq!(again["pendingSummary"], detail["pendingSummary"]);
}

/// No pool view, no pending summary: the key is present and `null`, never a
/// `0` that reads as "nothing pending".
#[tokio::test]
async fn address_pending_summary_is_null_without_a_healthy_mirror() {
    let addr = format!("/api/v1/addresses/0x{}", hex::encode(POOL_LOCK_HASH));

    let state = test_app_state(test_config(test_store()));
    state.pool_mirror.publish(unhealthy_pool_snapshot());
    let app = create_router_with_state(state).await;
    let (status, json) = fetch_json(app, &addr).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert!(
        json.as_object().unwrap().contains_key("pendingSummary"),
        "{json}"
    );
    assert!(json["pendingSummary"].is_null(), "{json}");

    let mut config = test_config(test_store());
    config.pool_mirror_enabled = false;
    let app = create_router(config).await;
    let (status, json) = fetch_json(app, &addr).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert!(
        json.as_object().unwrap().contains_key("pendingSummary"),
        "{json}"
    );
    assert!(json["pendingSummary"].is_null(), "{json}");

    // A healthy mirror with nothing for this address is a real zero.
    let state = test_app_state(test_config(test_store()));
    state.pool_mirror.publish(healthy_pool_snapshot(vec![]));
    let app = create_router_with_state(state).await;
    let (_, json) = fetch_json(app, &addr).await;
    assert_eq!(
        json["pendingSummary"],
        serde_json::json!({ "txCount": 0, "capacityDelta": "0" })
    );
}

/// The detail response is cached; the pending summary is not — a pool
/// transaction that arrives after the first request shows on the next one.
#[tokio::test]
async fn address_pending_summary_is_not_served_from_the_detail_cache() {
    let state = test_app_state(test_config(test_store()));
    state.pool_mirror.publish(healthy_pool_snapshot(vec![]));
    let app = create_router_with_state(state.clone()).await;
    let addr = format!("/api/v1/addresses/0x{}", hex::encode(POOL_LOCK_HASH));

    let (_, first) = fetch_json(app.clone(), &addr).await;
    assert_eq!(first["pendingSummary"]["txCount"], 0);

    state
        .pool_mirror
        .publish(healthy_pool_snapshot(vec![make_test_pool_record(
            &[0xf1; 32],
            &POOL_LOCK_HASH,
            700,
            1_700_000_500_000,
            ckbadger_api::pool::PoolStatus::Pending,
        )]));
    let (_, second) = fetch_json(app, &addr).await;
    assert_eq!(
        second["pendingSummary"],
        serde_json::json!({ "txCount": 1, "capacityDelta": "700" })
    );
}

// ── 5.9: one lock → address resolver ─────────────────────────────────────

/// Seed one committed tx at block 10 in which `lock_hash` pays `other`.
fn seed_two_party_tx(store: &Arc<CkbadgerStore>, lock_hash: &[u8; 32], other: &[u8; 32]) {
    use ckbadger_store::types::{ParticipantDelta, ParticipantId};
    let tx_hash = vec![0xaa; 32];
    let actions = TxActions {
        tx_hash: tx_hash.clone(),
        block_hash: vec![0xbb; 32],
        block_number: 10,
        tx_index: 0,
        timestamp: 1_700_000_000_000,
        is_cellbase: false,
        protocol_actions: vec![],
        type_calls: vec![],
        lock_calls: vec![],
        participants: vec![
            ParticipantDelta {
                id: ParticipantId::Lock(*lock_hash),
                ckb_delta: -100,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
                roles: 0,
            },
            ParticipantDelta {
                id: ParticipantId::Lock(*other),
                ckb_delta: 100,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
                roles: 0,
            },
        ],
    };
    seed_named_tx(store, 10, &tx_hash, &actions);
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_addr_tx(
        lock_hash,
        10,
        0,
        &tx_hash,
        &AddrTxValue::new(-100, true, false, 0),
    );
    batch.commit().unwrap();
}

/// A party whose lock script the store does not know is reported by its lock
/// hash with no address — never with the hex lock hash posing as an address.
#[tokio::test]
async fn unknown_lock_participant_is_unresolved_not_a_fabricated_address() {
    let store = test_store();
    let lock_hash = [0x42u8; 32];
    let other = [0x11u8; 32];
    seed_two_party_tx(&store, &lock_hash, &other);
    let app = create_router(test_config(store)).await;

    let (status, json) = get_json(
        &app,
        &format!("/addresses/0x{}/activities", hex::encode(lock_hash)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let party = &json["data"][0]["participants"][0];
    assert_eq!(party["lockHash"], format!("0x{}", hex::encode(other)));
    assert!(party["address"].is_null(), "{json}");
}

/// A lock script the store holds but cannot encode (a hash_type no chain
/// allows) is store corruption, reported with the lock — not rendered as a
/// guess or dropped.
#[tokio::test]
async fn corrupt_lock_script_entry_is_a_500_naming_the_lock() {
    let store = test_store();
    let lock_hash = [0x42u8; 32];
    let other = [0x11u8; 32];
    seed_two_party_tx(&store, &lock_hash, &other);
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_lock_script(
        &other,
        &ckbadger_store::types::LockScriptEntry {
            code_hash: vec![0x9b; 32],
            hash_type: 7,
            args: vec![0x11; 20],
        },
    );
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;

    let (status, json) = get_json(
        &app,
        &format!("/addresses/0x{}/activities", hex::encode(lock_hash)),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .contains(&hex::encode(other)),
        "{json}"
    );
}

/// A pending transaction paying a lock no committed cell has ever used: the
/// recipient's address comes from the script the pool transaction itself
/// carries, the same encoder the chain view uses, not from a store lookup
/// that cannot know the lock yet.
#[tokio::test]
async fn pending_tx_paying_a_never_seen_lock_shows_its_real_address() {
    use ckbadger_store::types::{ParticipantDelta, ParticipantId};

    let secp =
        hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8").unwrap();
    let args = vec![0x5e; 20];
    let cell = ckbadger_api::pool::ResolvedCell::new(
        100_000_000_000,
        secp.clone(),
        1,
        args.clone(),
        None,
        vec![],
    )
    .unwrap();
    let recipient: [u8; 32] = cell.lock_script_hash.clone().try_into().unwrap();
    let expected =
        ckbadger_api::utils::address::script_to_address(&secp, 1, &args, "mainnet").unwrap();

    let mut record = make_test_pool_record(
        &[0xf7; 32],
        &POOL_LOCK_HASH,
        -100_000_000_000,
        1_700_000_500_000,
        ckbadger_api::pool::PoolStatus::Pending,
    );
    record.outputs = vec![cell];
    record
        .actions
        .as_mut()
        .unwrap()
        .participants
        .push(ParticipantDelta {
            id: ParticipantId::Lock(recipient),
            ckb_delta: 100_000_000_000,
            used_delta: 0,
            item_deltas: vec![],
            tags: 0,
            roles: 0,
        });

    let mut config = test_config(test_store());
    config.ckb_network = "mainnet".to_string();
    let state = test_app_state(config);
    state
        .pool_mirror
        .publish(healthy_pool_snapshot(vec![record]));
    let app = create_router_with_state(state).await;

    let (status, json) = get_json(
        &app,
        &format!("/addresses/0x{}/activities", hex::encode(POOL_LOCK_HASH)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let row = &json["data"][0];
    assert_eq!(row["poolStatus"], "pending", "{json}");
    let party = &row["participants"][0];
    assert_eq!(party["lockHash"], format!("0x{}", hex::encode(recipient)));
    assert_eq!(party["address"], expected, "{json}");
}

// ── 2.6b: identity item deltas name their standard ──────────────────────

/// Every identity item delta carries its standard, as the builder recorded
/// it, in the standard's one wire value (`IdentityStandard::asset_standard`):
/// the frontend links the item to its page with it. No store lookup — the
/// four identities below exist nowhere in the store.
#[tokio::test]
async fn identity_item_deltas_name_their_standard() {
    use ckbadger_store::types::{
        ItemDelta, ItemKind, ParticipantDelta, ParticipantId, TAG_IDENTITY,
    };

    let store = test_store();
    let lock_hash = [0x42u8; 32];
    let tx_hash = vec![0xab; 32];
    let identity = |standard, byte| ItemDelta {
        item_id: vec![byte; 20],
        kind: ItemKind::Identity(standard),
        magnitude: 1,
        negative: false,
    };
    let actions = TxActions {
        tx_hash: tx_hash.clone(),
        block_hash: vec![0xbb; 32],
        block_number: 10,
        tx_index: 0,
        timestamp: 1_700_000_000_000,
        is_cellbase: false,
        protocol_actions: vec![],
        type_calls: vec![],
        lock_calls: vec![],
        participants: vec![ParticipantDelta {
            id: ParticipantId::Lock(lock_hash),
            ckb_delta: 0,
            used_delta: 0,
            item_deltas: vec![
                identity(IdentityStandard::DotBit, 0x01),
                identity(IdentityStandard::BitCell, 0x02),
                identity(IdentityStandard::DidCkb, 0x03),
                identity(IdentityStandard::DotCell, 0x04),
            ],
            tags: TAG_IDENTITY,
            roles: 0,
        }],
    };
    seed_named_tx(&store, 10, &tx_hash, &actions);
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_addr_tx(
        &lock_hash,
        10,
        0,
        &tx_hash,
        &AddrTxValue::new(0, true, true, TAG_IDENTITY),
    );
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;

    let (status, json) = get_json(
        &app,
        &format!("/addresses/0x{}/activities", hex::encode(lock_hash)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let deltas = json["data"][0]["itemDeltas"].as_array().unwrap();
    let standards: Vec<&str> = deltas
        .iter()
        .map(|d| d["standard"].as_str().unwrap())
        .collect();
    assert_eq!(
        standards,
        ["dotbit", "bit_cell", "did_ckb", "dotcell"],
        "{json}"
    );

    let (_, global) = get_json(&app, "/activities").await;
    assert_eq!(
        global["data"][0]["participants"][0]["itemDeltas"][3]["standard"], "dotcell",
        "{global}"
    );
}

/// A pending `.cell` registration: the name is not in the store yet, and the
/// pool row still names its standard — carried by the item delta the mirror's
/// builder produced, not looked up.
#[tokio::test]
async fn pending_dotcell_registration_item_delta_carries_its_standard() {
    use ckbadger_store::types::{ItemDelta, ItemKind, TAG_IDENTITY};

    let mut record = make_test_pool_record_with(
        &[0xf9; 32],
        &POOL_LOCK_HASH,
        -24_000_000_000,
        1_700_000_500_000,
        ckbadger_api::pool::PoolStatus::Pending,
        TAG_IDENTITY,
        ckbadger_api::pool::Interpretation::Complete,
    );
    let new_name = ckbadger_store::types::derive_dotcell_id("brandnew");
    record.actions.as_mut().unwrap().participants[0]
        .item_deltas
        .push(ItemDelta {
            item_id: new_name.to_vec(),
            kind: ItemKind::Identity(IdentityStandard::DotCell),
            magnitude: 1,
            negative: false,
        });

    let state = test_app_state(test_config(test_store()));
    state
        .pool_mirror
        .publish(healthy_pool_snapshot(vec![record]));
    let app = create_router_with_state(state).await;

    let (status, json) = get_json(
        &app,
        &format!("/addresses/0x{}/activities", hex::encode(POOL_LOCK_HASH)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let row = &json["data"][0];
    assert_eq!(row["poolStatus"], "pending", "{json}");
    assert_eq!(row["itemDeltas"][0]["kind"], "identity");
    assert_eq!(
        row["itemDeltas"][0]["identityId"],
        format!("0x{}", hex::encode(new_name))
    );
    assert_eq!(row["itemDeltas"][0]["standard"], "dotcell", "{json}");
}
