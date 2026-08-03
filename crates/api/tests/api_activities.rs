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

    use ckbadger_store::types::{ParticipantDelta, TAG_TYPE_CALL};
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
            lock_hash: lock_hash.clone(),
            ckb_delta: 0,
            used_delta: 0,
            item_deltas: vec![],
            tags: TAG_TYPE_CALL,
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
    use ckbadger_store::types::{ParticipantDelta, TAG_DAO, TAG_TYPE_CALL};
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
            lock_hash: vec![0x13; 32],
            ckb_delta: -30000,
            used_delta: 0,
            item_deltas: vec![],
            tags: TAG_TYPE_CALL | TAG_DAO,
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
