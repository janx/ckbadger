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
